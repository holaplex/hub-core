use std::{collections::HashMap, io::prelude::*, path::PathBuf};

pub use hub_core_schemas::credits::Action;
use hub_core_schemas::{credits, credits_mpsc};
use strum::IntoEnumIterator;

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
    #[inline]
    pub async fn build<I: LineItem>(self) -> Result<CreditsClient<I>> {
        CreditsClient::new(self).await
    }
}

/// A client for producing credit deduction line items
#[derive(Debug, Clone)]
pub struct CreditsClient<I> {
    credit_sheet: Arc<CreditSheet<I>>,
    producer: producer::Producer<credits_mpsc::CreditsMpscEvent>,
}

pub type CreditSheet<I> = HashMap<(I, Blockchain), u64>;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter, strum::AsRefStr, strum::Display,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Blockchain {
    OffChain,
    Solana,
    Polygon,
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
pub struct TransactionId {
    _stub: PhantomData<()>, // TODO: stub
}

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
            credit_sheet: Arc::new(
                I::iter()
                    .flat_map(|item| {
                        let action = item.as_ref();

                        let toml = toml.get(action).unwrap_or_else(|| unreachable!());

                        Blockchain::iter()
                            .filter_map(move |b| toml.get(b.as_ref()).map(|c| ((item, b), *c)))
                    })
                    .collect(),
            ),
            producer: producer::Config {
                topic: "credits_mpsc".into(),
                config,
            }
            .build()
            .await?,
        })
    }

    #[inline]
    #[must_use]
    pub fn credit_sheet(&self) -> &CreditSheet<I> {
        &self.credit_sheet
    }

    #[inline]
    pub fn get_cost<Q: fmt::Debug + Eq + std::hash::Hash + ?Sized>(&self, key: &Q) -> Result<u64>
    where
        (I, Blockchain): Borrow<Q>,
    {
        self.credit_sheet
            .get(key)
            .ok_or_else(|| anyhow!("No price in credit sheet found for key {key:?}"))
            .copied()
    }

    #[inline]
    pub async fn submit_pending_deduction(
        &self,
        organization_id: String,
        user_id: String,
        item: I,
        blockchain: Blockchain,
    ) -> Result<TransactionId> {
        let credits = self
            .get_cost(&(item, blockchain))?
            .try_into()
            .context("Credit price was too big to transmit")?;

        self.producer
            .send(
                Some(&credits_mpsc::CreditsMpscEvent {
                    event: Some(credits_mpsc::credits_mpsc_event::Event::DeductCredits(
                        credits::Credits {
                            credits,
                            action: item.into().into(),
                            blockchain: credits::Blockchain::from(blockchain).into(),
                        },
                    )),
                }),
                Some(&credits::CreditsEventKey {
                    id: organization_id,
                    user_id,
                }),
            )
            .await?;

        Ok(TransactionId {
            _stub: PhantomData::default(),
        })
    }

    pub async fn confirm_deduction(&self, id: TransactionId) -> Result<()> {
        todo!("stub")
    }
}
