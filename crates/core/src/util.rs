//! Miscellaneous core utilities

use std::fmt;

/// A zero-cost wrapper that implements [`Debug`](fmt::Debug) for values that
/// have no `Debug` implementation
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct DebugShim<T>(pub T);

impl<T> fmt::Debug for DebugShim<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(std::any::type_name::<T>())
            .finish_non_exhaustive()
    }
}

impl<T> From<T> for DebugShim<T> {
    fn from(val: T) -> Self {
        Self(val)
    }
}
