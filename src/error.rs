//! The [`InternError`] type returned by fallible interning.

use core::fmt;

/// An error returned when a string cannot be interned.
///
/// Interning a *new* string consumes one symbol from a finite space. When that
/// space is exhausted there is no symbol left to assign, and
/// [`try_intern`](crate::Interner::try_intern) reports it with this error rather
/// than panicking or aliasing the string onto an existing symbol.
///
/// This enum is `#[non_exhaustive]`: future releases may add variants for new
/// failure modes without it being a breaking change, so a `match` on it must
/// include a wildcard arm.
///
/// # Examples
///
/// ```
/// use intern_lang::{InternError, Interner};
///
/// let mut interner = Interner::new();
/// // Far below the symbol-space bound, interning always succeeds.
/// assert!(interner.try_intern("name").is_ok());
///
/// fn describe(err: InternError) -> &'static str {
///     match err {
///         InternError::SymbolSpaceExhausted => "out of symbols",
///         _ => "unknown interning error",
///     }
/// }
/// assert_eq!(describe(InternError::SymbolSpaceExhausted), "out of symbols");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InternError {
    /// The symbol space is exhausted: the interner has already issued all
    /// `u32::MAX` symbols, so no new string can be assigned one.
    ///
    /// Reaching this requires interning over four billion *distinct* strings,
    /// which exhausts memory on any normal machine long before the symbol space.
    /// A caller that must handle the boundary explicitly — rather than relying on
    /// it being unreachable — should use
    /// [`try_intern`](crate::Interner::try_intern) and treat this variant as a
    /// hard stop: the interner cannot accept further distinct strings.
    SymbolSpaceExhausted,
}

impl fmt::Display for InternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SymbolSpaceExhausted => {
                f.write_str("symbol space exhausted: the interner has issued all u32::MAX symbols")
            }
        }
    }
}

impl core::error::Error for InternError {}
