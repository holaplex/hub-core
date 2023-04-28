use std::{collections::HashMap, io::prelude::*, path::PathBuf};

pub use hub_core_schemas::credits_mpsc::credits_mpsc_event::Event;
use hub_core_schemas::{credits::CreditsEventKey, credits_mpsc::CreditsMpscEvent};

use crate::{prelude::*, producer, util::DebugShim};

impl producer::Message for CreditsMpscEvent {
    type Key = CreditsEventKey;
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
    producer: producer::Producer<CreditsMpscEvent>,
}

pub type CreditSheet<I> = HashMap<I, u64>;

#[derive(Debug, Clone, Copy, strum::AsRefStr, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Blockchain {
    Solana,
}

pub trait LineItem:
    fmt::Debug + Eq + std::hash::Hash + strum::IntoEnumIterator + Into<Event> + 'static
{
    fn action_and_blockchain(&self) -> (&'static str, Blockchain);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

        Ok(Self {
            credit_sheet: Arc::new(
                I::iter()
                    .map(|item| {
                        let (action, blockchain) = item.action_and_blockchain();

                        toml.get(action)
                            .and_then(|map| map.get(blockchain.as_ref()))
                            .ok_or_else(|| {
                                anyhow!(
                                    "Missing entry in credit sheet for {action:?} on {blockchain:?}"
                                )
                            })
                            .map(|i| (item, *i))
                    })
                    .collect::<Result<_>>()?,
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
    pub fn get_cost<Q: fmt::Debug + Eq + std::hash::Hash + ?Sized>(&self, item: &Q) -> Result<u64>
    where
        I: Borrow<Q>,
    {
        self.credit_sheet
            .get(item)
            .ok_or_else(|| anyhow!("No price associated with credit line item {item:?}"))
            .copied()
    }

    #[inline]
    pub async fn submit_pending_deduction(
        &self,
        organization_id: String,
        user_id: String,
        item: I,
    ) -> Result<TransactionId> {
        let credits = self.get_cost(&item)?;

        self.producer
            .send(
                Some(&CreditsMpscEvent {
                    event: Some(item.into()),
                }),
                Some(&CreditsEventKey {
                    id: organization_id,
                    user_id,
                }),
            )
            .await?;

        Ok(TransactionId { _stub: PhantomData::default() })
    }

    pub async fn confirm_deduction(&self, id: TransactionId) -> Result<()> {
        todo!("stub")
    }
}
