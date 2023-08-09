//! Support macros for `holaplex-hub-core`

#![deny(
    clippy::disallowed_methods,
    clippy::suspicious,
    clippy::style,
    missing_debug_implementations,
    missing_copy_implementations
)]
#![warn(clippy::pedantic, clippy::cargo, missing_docs)]
#![allow(clippy::module_name_repetitions)]

use proc_macro::TokenStream as TokenStream1;

mod triage;

pub(crate) mod prelude {
    pub use proc_macro2::{Span, TokenStream};
    pub use quote::{quote_spanned, ToTokens, TokenStreamExt};
    pub use syn::spanned::Spanned;

    pub trait SpanExt {
        fn error(self, msg: impl std::fmt::Display) -> syn::Error;
    }

    impl<T: Into<Span>> SpanExt for T {
        #[inline]
        fn error(self, msg: impl std::fmt::Display) -> syn::Error {
            syn::Error::new(self.into(), msg)
        }
    }
}

/// Derive the `Triage` trait for a struct or enum
#[proc_macro_derive(Triage, attributes(transient, permanent, fatal, source, from))]
pub fn triage(input: TokenStream1) -> TokenStream1 {
    triage::run(syn::parse_macro_input!(input)).into()
}
