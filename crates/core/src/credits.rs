use std::{collections::HashMap, io::prelude::*, path::PathBuf};

use hub_core_schemas::{CreditsEvent, CreditsEventKey};

use crate::{prelude::*, producer, util::DebugShim};

impl producer::Message for CreditsEvent {
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
#[derive(Debug)]
pub struct CreditsClient<I> {
    credit_sheet: CreditSheet<I>,
    producer: producer::Producer<CreditsEvent>,
}

pub type LineItemDesc<T> = (&'static str, &'static str, T);
pub type CreditSheet<I> = HashMap<I, u64>;

// TODO: refactor this to something less stupid
pub trait LineItem: fmt::Debug + Copy + Eq + std::hash::Hash + 'static {
    /// Tuples of (action name, blockchain, corresponding line item
    const LIST: &'static [LineItemDesc<Self>];
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
            credit_sheet: I::LIST
                .iter()
                .copied()
                .map(|(action, blockchain, item)| {
                    toml.get(action)
                        .and_then(|map| map.get(blockchain))
                        .ok_or_else(|| {
                            anyhow!(
                                "Missing entry in credit sheet for {action:?} on {blockchain:?}"
                            )
                        })
                        .map(|i| (item, *i))
                })
                .collect::<Result<_>>()?,
            producer: producer::Config {
                topic: "creditsmpsc".into(), // TODO: hyphenate?
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
    pub async fn send_line_item<Q: fmt::Debug + Eq + std::hash::Hash + ?Sized>(
        &self,
        item: &Q,
    ) -> Result<()>
    where
        I: Borrow<Q>,
    {
        let credits = self.get_cost(item)?;

        self.producer
            .send(Some(&CreditsEvent {}), Some(&CreditsEventKey {}))
            .await
            .map_err(Into::into)
    }
}
