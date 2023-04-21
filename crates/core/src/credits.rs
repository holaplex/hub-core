use std::{collections::HashMap, io::prelude::*, path::PathBuf};

use crate::prelude::*;

/// Service startup configuration for charging credits and reading the credit
/// sheet
#[derive(Debug)]
pub struct Config {
    pub(crate) credit_sheet: PathBuf,
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
    credit_sheet: HashMap<I, u64>,
}

pub type LineItemDesc<T> = (&'static str, &'static str, T);

// TODO: refactor this to something less stupid
pub trait LineItem: Copy + Eq + std::hash::Hash + 'static {
    /// Tuples of (action name, blockchain, corresponding line item
    const LIST: &'static [LineItemDesc<Self>];
}

impl<I: LineItem> CreditsClient<I> {
    pub(crate) async fn new(config: Config) -> Result<Self> {
        let Config { credit_sheet } = config;
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
        })
    }
}
