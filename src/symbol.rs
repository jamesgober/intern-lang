//! The [`Symbol`] handle: a small, copyable stand-in for an interned string.

use core::fmt;
use core::num::NonZeroU32;

/// A small, copyable handle to a string held by an [`Interner`](crate::Interner).
///
/// A `Symbol` is a newtype over a 32-bit id, so it is four bytes, `Copy`, and as
/// cheap to pass or store as an integer. The point of interning is that a name in
/// an AST node or a symbol-table key costs a `Symbol` instead of an owned
/// `String`, and comparing two names becomes an integer comparison instead of a
/// byte-by-byte walk — `==`, `Ord`, and `Hash` on a `Symbol` never touch the
/// underlying bytes.
///
/// A `Symbol` is only meaningful together with the interner that issued it. Two
/// symbols from the *same* interner are equal exactly when they name the same
/// string; symbols from different interners share no relationship and should not
/// be compared or resolved across interners (resolving a foreign symbol is
/// handled safely — see [`Interner::resolve`](crate::Interner::resolve) — but the
/// result is not meaningful).
///
/// # Examples
///
/// ```
/// use intern_lang::Interner;
///
/// let mut interner = Interner::new();
/// let a = interner.intern("loop");
/// let b = interner.intern("loop");
///
/// // Same string interns to the same symbol, so equality is an integer compare.
/// assert_eq!(a, b);
///
/// // A symbol is `Copy`: handing it around never moves or clones a string.
/// let copy = a;
/// assert_eq!(copy, a);
/// ```
///
/// # Serialization
///
/// With the `serde` feature enabled, `Symbol` serializes transparently as its raw
/// integer id (the same value [`as_u32`](Symbol::as_u32) returns) and
/// deserializes back, rejecting `0`. The id is only meaningful with the interner
/// that issued it, so persist symbols alongside the interner that produced them.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(NonZeroU32);

impl Symbol {
    /// Reconstructs a symbol from a raw 1-based id, or `None` if `id` is `0`.
    ///
    /// This is the inverse of [`as_u32`](Symbol::as_u32), for rebuilding a symbol
    /// from an id stored elsewhere (a file, a wire message, an external table).
    /// It only rebuilds the handle; whether the id names anything is decided when
    /// you [`resolve`](crate::Interner::resolve) it against an interner, which
    /// returns `None` for an out-of-range id. A symbol is only meaningful with the
    /// interner that issued it.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::{Interner, Symbol};
    ///
    /// let mut interner = Interner::new();
    /// let sym = interner.intern("persisted");
    ///
    /// // Round-trip a symbol through its raw id.
    /// let rebuilt = Symbol::from_u32(sym.as_u32()).unwrap();
    /// assert_eq!(rebuilt, sym);
    /// assert_eq!(interner.resolve(rebuilt), Some("persisted"));
    ///
    /// // Zero is never a valid symbol id.
    /// assert_eq!(Symbol::from_u32(0), None);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_u32(id: u32) -> Option<Self> {
        NonZeroU32::new(id).map(Self)
    }

    /// Builds a symbol from a 1-based id.
    ///
    /// The interner only ever calls this with `id >= 1` (ids are assigned
    /// sequentially starting at one), so the `NonZeroU32` construction always
    /// succeeds; the saturating fallback exists solely to keep this path free of
    /// `unwrap`/`expect` and can never be reached with a valid id.
    #[inline]
    pub(crate) fn from_raw(id: u32) -> Self {
        Self(NonZeroU32::new(id).unwrap_or(NonZeroU32::MIN))
    }

    /// Returns the 0-based index this symbol occupies in the issuing interner's
    /// storage. Used internally to look the string back up.
    #[inline]
    pub(crate) fn index(self) -> usize {
        // `get()` is in `1..=u32::MAX`, so the subtraction never underflows.
        self.0.get() as usize - 1
    }

    /// Returns the raw 1-based integer id backing this symbol.
    ///
    /// The id is stable for the lifetime of the issuing interner and is assigned
    /// in interning order: the first distinct string interned gets `1`, the
    /// second `2`, and so on. It is exposed for diagnostics, stable ordering, and
    /// building external side tables keyed by symbol; it is not a string and
    /// carries no meaning outside the interner that produced it.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let mut interner = Interner::new();
    /// let first = interner.intern("alpha");
    /// let second = interner.intern("beta");
    ///
    /// assert_eq!(first.as_u32(), 1);
    /// assert_eq!(second.as_u32(), 2);
    /// // Re-interning an existing string returns the same id, never a new one.
    /// assert_eq!(interner.intern("alpha").as_u32(), 1);
    /// ```
    #[inline]
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0.get()
    }
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Symbol({})", self.0.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_raw_roundtrips_through_as_u32() {
        for id in [1u32, 2, 7, 1000, u32::MAX] {
            assert_eq!(Symbol::from_raw(id).as_u32(), id);
        }
    }

    #[test]
    fn test_index_is_zero_based() {
        assert_eq!(Symbol::from_raw(1).index(), 0);
        assert_eq!(Symbol::from_raw(42).index(), 41);
    }

    #[test]
    fn test_symbol_is_four_bytes() {
        assert_eq!(core::mem::size_of::<Symbol>(), 4);
    }

    #[test]
    fn test_option_symbol_is_niche_optimized() {
        // `NonZeroU32` lets `Option<Symbol>` reuse the zero bit pattern, so it
        // stays four bytes — a property serde and external side tables rely on.
        assert_eq!(core::mem::size_of::<Option<Symbol>>(), 4);
    }

    #[test]
    fn test_equal_ids_compare_equal() {
        assert_eq!(Symbol::from_raw(5), Symbol::from_raw(5));
        assert_ne!(Symbol::from_raw(5), Symbol::from_raw(6));
    }

    #[test]
    fn test_debug_shows_id() {
        extern crate alloc;
        assert_eq!(alloc::format!("{:?}", Symbol::from_raw(9)), "Symbol(9)");
    }
}
