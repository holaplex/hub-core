use std::borrow::Cow;

/// Given a comma-separated list of field names, generate an
/// `ActiveModelBehavior::before_save` method that converts any EVM-address-like
/// strings stored in the listed fields to lowercase.  Field names suffixed with
/// a question mark `?` will be treated as if wrapped in an `Option<T>`.
///
/// An "EVM-address-like" string is any string that begins with the prefix `0x`.
#[macro_export]
macro_rules! before_save_evm_addrs {
    (@body $self:ident;) => {};

    (@body $self:ident; $ident:ident $(, $($rest:tt)*)?) => {
        if $self.$ident.as_ref().starts_with("0x") {
            $self.$ident = ::sea_orm::Set($self.$ident.as_ref().to_lowercase());
        }

        $crate::before_save_evm_addrs!(@body $self; $($($rest)*)?)
    };

    (@body $self:ident; $ident:ident? $(, $($rest:tt)*)?) => {
        match $self.$ident.as_ref() {
            Some(s) if s.starts_with("0x") => {
                $self.$ident = ::sea_orm::Set(Some(s.to_lowercase()))
            },
            _ => (),
        }

        $crate::before_save_evm_addrs!(@body $self; $($($rest)*)?)
    };

    ($($tts:tt)*) => {
        fn before_save(mut self, _: bool) -> ::std::result::Result<Self, ::sea_orm::DbErr> {
            $crate::before_save_evm_addrs!(@body self; $($tts)*);

            Ok(self)
        }
    };
}

/// Convert any EVM-adddress-like strings in the iterator to lowercase
///
/// An "EVM-address-like" string is any string that begins with the prefix `0x`.
pub fn downcase_evm_addresses<'a, I: IntoIterator>(it: I) -> impl Iterator<Item = Cow<'a, str>>
where
    I::Item: Into<Cow<'a, str>>,
{
    it.into_iter().map(Into::into).map(|s| {
        if s.starts_with("0x") {
            Cow::Owned(s.to_lowercase())
        } else {
            s
        }
    })
}
