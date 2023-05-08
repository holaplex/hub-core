use anyhow::Result;
use cid::Cid;
use strum;
use url::Url;

use crate::prelude::*;
/// Supported width sizes for asset proxy
#[derive(Debug, Clone, Copy, strum::AsRefStr)]
#[repr(i32)]
pub enum ImageSize {
    /// image natural size
    #[strum(serialize = "0")]
    Original,
    /// tiny image
    #[strum(serialize = "100")]
    Tiny,
    /// extra small image
    #[strum(serialize = "400")]
    XSmall,
    /// small image
    #[strum(serialize = "600")]
    Small,
    /// medium image
    #[strum(serialize = "800")]
    Medium,
    /// large image
    #[strum(serialize = "1000")]
    Large,
}

#[derive(Debug, Clone)]
pub struct AssetProxy {
    assets_cdn: Url,
}

impl AssetProxy {
    pub(crate) fn new(cdn: &str) -> Result<Self> {
        let assets_cdn = Url::parse(cdn)?;
        Ok(Self { assets_cdn })
    }

    pub fn proxy_ipfs_image(&mut self, url: &Url, size: Option<ImageSize>) -> Result<Option<Url>> {
        let mut res = Ok(None);

        visit_url(url, |s, i| {
            let slice_path = || {
                i.and_then(|i| url.path_segments().map(|s| (i, s)))
                    .map_or_else(String::new, |(i, s)| {
                        s.skip(i).collect::<Vec<_>>().join("/")
                    })
            };
            if let Ok(cid) = s.parse::<Cid>() {
                advance_heuristic(&mut res, (cid, slice_path()));
            }
        });

        if let Ok(Some((cid, path))) = res.as_ref() {
            let proxy_url = &mut self.assets_cdn;
            proxy_url
                .path_segments_mut()
                .map_err(|_| anyhow!("invalid url"))?
                .extend(&["ipfs", &cid.to_string()]);

            proxy_url
                .query_pairs_mut()
                .append_pair("width", size.unwrap_or(ImageSize::Original).as_ref());

            if !path.is_empty() {
                proxy_url.query_pairs_mut().append_pair("path", path);
            }

            return Ok(Some(proxy_url.clone()));
        }

        Ok(None)
    }
}

fn advance_heuristic<T: Eq>(state: &mut Result<Option<T>, ()>, value: T) {
    match state {
        // We found a match
        Ok(None) => *state = Ok(Some(value)),
        // We found two identical matches, no change is necessary
        Ok(Some(v)) if *v == value => (),
        // We found two differing matches, convert to error due to ambiguity
        Ok(Some(_)) => *state = Err(()),
        Err(()) => (),
    }
}

fn visit_url(url: &Url, mut f: impl FnMut(&str, Option<usize>)) {
    Some(url.scheme())
        .into_iter()
        .chain(url.domain().unwrap_or_default().split('.'))
        .chain(Some(url.username()))
        .chain(url.password())
        .map(|s| (s, Some(0)))
        .chain(Some((url.path(), None)))
        .chain(
            url.path_segments()
                .into_iter()
                .flat_map(|s| s.into_iter().enumerate().map(|(i, s)| (s, Some(i + 1)))),
        )
        .chain(url.query().map(|q| (q, Some(0))))
        .chain(url.fragment().map(|f| (f, Some(0))))
        .for_each(|(s, i)| f(s, i));

    url.query_pairs().for_each(|(k, v)| {
        f(k.as_ref(), Some(0));
        f(v.as_ref(), Some(0));
    });
}
