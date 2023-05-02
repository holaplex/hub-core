use std::{collections::HashMap, io::prelude::*, path::PathBuf};

pub use hub_core_schemas::credits::Action;
use hub_core_schemas::{credits, credits_mpsc};
use rand::prelude::*;
use strum::IntoEnumIterator;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{prelude::*, producer, util::DebugShim};

impl producer::Message for credits_mpsc::CreditsMpscEvent {
    type Key = credits::CreditsEventKey;
}

/// Service startup configuration for charging credits and reading the credit
/// sheet
#[derive(Debug)]
pub struct Config {
    pub(crate) credit_sheet: PathBuf,
    pub(crate) config: DebugShim<rdkafka::ClientConfig>,
}

impl Config {
    /// Construct a new credits producer client from this config instance
    ///
    /// # Errors
    /// This method returns an error if the service configuration given is
    /// invalid or the client fails to initialize.
    #[inline]
    pub async fn build<I: LineItem>(self) -> Result<CreditsClient<I>> {
        CreditsClient::new(self).await
    }
}

/// A client for producing credit deduction line items
#[derive(Debug, Clone)]
// Default parameter used as a static assert that StdRng is a CSPRNG
pub struct CreditsClient<I, R: CryptoRng + SeedableRng = StdRng> {
    producer: producer::Producer<credits_mpsc::CreditsMpscEvent>,
    core: Arc<Core<I, R>>,
}

#[derive(Debug)]
struct Core<I, R> {
    credit_sheet: CreditSheet<I>,
    rng: Mutex<R>,
}

/// The type of the underlying map between actions and credit costs
pub type CreditSheet<I> = HashMap<(I, Blockchain), u64>;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter, strum::AsRefStr, strum::Display,
)]
#[strum(serialize_all = "kebab-case")]
/// Enum describing possible blockchains associated with an action
pub enum Blockchain {
    /// This action is not specific to a blockchain
    OffChain,
    /// This action uses Solana
    Solana,
    /// This action uses Polygon
    Polygon,
    /// This action uses Ethereum
    Ethereum,
}

impl From<Blockchain> for credits::Blockchain {
    fn from(value: Blockchain) -> Self {
        match value {
            Blockchain::OffChain => credits::Blockchain::Unspecified,
            Blockchain::Solana => credits::Blockchain::Solana,
            Blockchain::Polygon => credits::Blockchain::Polygon,
            Blockchain::Ethereum => credits::Blockchain::Ethereum,
        }
    }
}

/// Trait alias for an enum describing all actions for which a service may
/// charge credits
pub trait LineItem:
    fmt::Debug + Copy + Eq + std::hash::Hash + AsRef<str> + IntoEnumIterator + Into<Action> + 'static
{
}

impl<
    T: fmt::Debug
        + Copy
        + Eq
        + std::hash::Hash
        + AsRef<str>
        + IntoEnumIterator
        + Into<Action>
        + 'static,
> LineItem for T
{
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[must_use = "Any created transactions should be confirmed or they will be discarded"]
/// An ID for a credit transaction
pub struct TransactionId(pub Uuid);

impl<I: LineItem> CreditsClient<I> {
    pub(crate) async fn new(config: Config) -> Result<Self> {
        let Config {
            credit_sheet,
            config,
        } = config;
        let mut file =
            std::fs::File::open(credit_sheet).context("Error opening credit sheet file")?;
        let mut s = String::new();
        file.read_to_string(&mut s)
            .context("Error reading credit sheet file")?;
        let toml: HashMap<String, HashMap<String, u64>> =
            toml::from_str(&s).context("Syntax error in credit sheet")?;

        for item in I::iter() {
            if !toml.contains_key(item.as_ref()) {
                bail!("Missing entry in credit sheet for {item:?}");
            }
        }

        Ok(Self {
            producer: producer::Config {
                topic: "credits_mpsc".into(),
                config,
            }
            .build()
            .await?,
            core: Core {
                credit_sheet: I::iter()
                    .flat_map(|item| {
                        let action = item.as_ref();

                        let toml = toml.get(action).unwrap_or_else(|| unreachable!());

                        Blockchain::iter()
                            .filter_map(move |b| toml.get(b.as_ref()).map(|c| ((item, b), *c)))
                    })
                    .collect(),

                rng: Mutex::new(SeedableRng::from_entropy()),
            }
            .into(),
        })
    }

    /// Borrow the underlying credit price sheet for this service's actions
    #[inline]
    #[must_use]
    pub fn credit_sheet(&self) -> &CreditSheet<I> {
        &self.core.credit_sheet
    }

    /// Look up the cost of a given `(action, blockchain)` pair in credits
    ///
    /// # Errors
    /// This method returns an error if no price is found for the given input.
    #[inline]
    pub fn get_cost<Q: fmt::Debug + Eq + std::hash::Hash + ?Sized>(&self, key: &Q) -> Result<u64>
    where
        (I, Blockchain): Borrow<Q>,
    {
        self.core
            .credit_sheet
            .get(key)
            .ok_or_else(|| anyhow!("No price in credit sheet found for key {key:?}"))
            .copied()
    }

    /// Generate a new transaction ID and submit a pending transaction with it
    /// using the given transaction details
    ///
    /// # Errors
    /// This method returns an error if the associated credit cost of the action
    /// cannot be found or if transmitting the pending transaction fails.
    pub async fn submit_pending_deduction(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        item: I,
        blockchain: Blockchain,
    ) -> Result<TransactionId> {
        let credits = self
            .get_cost(&(item, blockchain))?
            .try_into()
            .context("Credit price was too big to transmit")?;

        #[allow(clippy::cast_sign_loss)]
        let ts = chrono::Utc::now().timestamp_millis() as u64;
        let txid = Uuid::from_u64_pair(ts, self.core.rng.lock().await.gen());

        self.producer
            .send(
                Some(&credits_mpsc::CreditsMpscEvent {
                    event: Some(credits_mpsc::credits_mpsc_event::Event::PendingDeduction(
                        credits::Credits {
                            credits,
                            action: item.into().into(),
                            blockchain: credits::Blockchain::from(blockchain).into(),
                            organization: organization_id.to_string(),
                        },
                    )),
                }),
                Some(&credits::CreditsEventKey {
                    id: txid.to_string(),
                    user_id: user_id.to_string(),
                }),
            )
            .await
            .context("Error sending pending transaction event")?;

        Ok(TransactionId(txid))
    }

    /// Submit a confirmation of the transaction with the given ID
    ///
    /// # Errors
    /// This method returns an error if transmitting the confirmation fails.
    #[inline]
    pub async fn confirm_deduction(&self, id: TransactionId) -> Result<()> {
        self.producer
            .send(
                Some(&credits_mpsc::CreditsMpscEvent {
                    event: Some(credits_mpsc::credits_mpsc_event::Event::ConfirmDeduction(
                        credits::Credits::default(),
                    )),
                }),
                Some(&credits::CreditsEventKey {
                    id: id.0.to_string(),
                    user_id: String::new(),
                }),
            )
            .await
            .context("Error sending confirm event")
    }
}
