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
        $crate::util::NormalizeAddress::normalize_address(&mut $self.$ident);

        $crate::before_save_evm_addrs!(@body $self; $($($rest)*)?)
    };

    (@body $self:ident; $ident:ident? $(, $($rest:tt)*)?) => {
        {
            #[deprecated = "The `field?` syntax for before_save_evm_addrs is deprecated"]
            fn __depr() {}
            __depr();
        }

        $crate::before_save_evm_addrs!(@body $self; $ident $($($rest)*)?)
    };

    ($($tts:tt)*) => {
        fn before_save<'a, 'b, C>(
            mut self,
            db: &'a C,
            _: bool,
        ) -> ::core::pin::Pin<
            Box<dyn ::std::future::Future<Output = ::std::result::Result<Self, ::sea_orm::DbErr>> + Send + 'b>,
        >
        where
            C: ::sea_orm::ConnectionTrait + 'b,
            Self: Send + 'b,
            'a: 'b,
        {
            Box::pin(async {
                $crate::before_save_evm_addrs!(@body self; $($tts)*);
                Ok(self)
            })
        }
    };
}

#[inline]
fn normalize_address_helper<S: AsRef<str>, F: FnOnce(S, String)>(val: S, f: F) {
    let s = val.as_ref();
    if s.starts_with("0x") {
        let lower = s.to_lowercase();
        f(val, lower);
    }
}

/// Consume a value and produce its equivalent with any contained EVM addresses
/// converted to lowercase
pub trait IntoNormalizedAddress {
    /// The resulting type of the value after normalization
    type Output;

    /// Consume `self` and produce a new value with all EVM addresses normalized
    fn into_normalized_address(self) -> Self::Output;
}

/// Similar to [`IntoNormalizedAddress`], but operates in-place
pub trait NormalizeAddress {
    /// Normalize any EVM addresses contained within `self`
    fn normalize_address(&mut self);
}

impl<'a> IntoNormalizedAddress for Cow<'a, str> {
    type Output = Cow<'a, str>;

    #[inline]
    fn into_normalized_address(mut self) -> Self::Output {
        self.normalize_address();
        self
    }
}

impl<'a> NormalizeAddress for Cow<'a, str> {
    fn normalize_address(&mut self) {
        normalize_address_helper(self, |me, new| *me = Cow::Owned(new));
    }
}

impl IntoNormalizedAddress for String {
    type Output = String;

    #[inline]
    fn into_normalized_address(mut self) -> Self::Output {
        self.normalize_address();
        self
    }
}

impl NormalizeAddress for String {
    fn normalize_address(&mut self) {
        normalize_address_helper(self, |me, new| *me = new);
    }
}

impl<'a, T: AsRef<str>> IntoNormalizedAddress for &'a T {
    type Output = Cow<'a, str>;

    #[inline]
    fn into_normalized_address(self) -> Self::Output {
        Cow::from(self.as_ref()).into_normalized_address()
    }
}

impl<T: IntoNormalizedAddress> IntoNormalizedAddress for Option<T> {
    type Output = Option<T::Output>;

    #[inline]
    fn into_normalized_address(self) -> Self::Output {
        self.map(IntoNormalizedAddress::into_normalized_address)
    }
}

impl<T: NormalizeAddress> NormalizeAddress for Option<T> {
    #[inline]
    fn normalize_address(&mut self) {
        if let Some(ref mut val) = self {
            val.normalize_address();
        }
    }
}

#[cfg(feature = "sea-orm")]
impl<T: IntoNormalizedAddress> IntoNormalizedAddress for sea_orm::ActiveValue<T>
where
    sea_orm::Value: From<T> + From<T::Output>,
{
    type Output = sea_orm::ActiveValue<T::Output>;

    fn into_normalized_address(self) -> Self::Output {
        use sea_orm::ActiveValue;

        match self {
            ActiveValue::Set(v) => ActiveValue::Set(v.into_normalized_address()),
            ActiveValue::Unchanged(v) => ActiveValue::Unchanged(v.into_normalized_address()),
            ActiveValue::NotSet => ActiveValue::NotSet,
        }
    }
}

#[cfg(feature = "sea-orm")]
impl<T: NormalizeAddress> NormalizeAddress for sea_orm::ActiveValue<T>
where
    sea_orm::Value: From<T>,
{
    fn normalize_address(&mut self) {
        use sea_orm::ActiveValue;

        match self {
            ActiveValue::Set(v) | ActiveValue::Unchanged(v) => v.normalize_address(),
            ActiveValue::NotSet => (),
        }
    }
}

/// Convert any EVM-adddress-like strings in the iterator to lowercase
///
/// An "EVM-address-like" string is any string that begins with the prefix `0x`.
pub fn downcase_evm_addresses<I: IntoIterator>(
    it: I,
) -> impl Iterator<Item = <I::Item as IntoNormalizedAddress>::Output>
where
    I::Item: IntoNormalizedAddress,
{
    it.into_iter()
        .map(IntoNormalizedAddress::into_normalized_address)
}
