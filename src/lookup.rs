//! The [`Lookup`] trait: the read-side contract shared by every interner.

use crate::symbol::Symbol;

/// The read-side contract every interner in the crate implements.
///
/// `Lookup` is the seam that lets generic code accept either the single-threaded
/// [`Interner`](crate::Interner) or the
/// [`ConcurrentInterner`](crate::ConcurrentInterner): both look a string up to its
/// symbol, resolve a symbol back to its bytes, and report their size — all through
/// `&self`. Interning is deliberately *not* on this trait, because the two
/// interners disagree on how it is called (`&mut self` for the single-threaded
/// one, `&self` for the concurrent one), and forcing a common signature would tax
/// the single-threaded hot path with synchronisation it does not need.
///
/// Resolution is expressed as [`resolve_with`](Lookup::resolve_with) — a closure
/// that runs against the borrowed string — rather than a method returning
/// `&str`. A concurrent interner can only expose the bytes while it holds its
/// read lock, so the borrow cannot outlive the call; a closure keeps resolution
/// zero-copy for both interners without leaking the lock's lifetime into the
/// signature.
///
/// # Examples
///
/// ```
/// use intern_lang::{Interner, Lookup};
///
/// fn name_len(interner: &impl Lookup, s: &str) -> Option<usize> {
///     let sym = interner.get(s)?;
///     interner.resolve_with(sym, str::len)
/// }
///
/// let mut interner = Interner::new();
/// let _ = interner.intern("identifier");
/// assert_eq!(name_len(&interner, "identifier"), Some(10));
/// assert_eq!(name_len(&interner, "missing"), None);
/// ```
pub trait Lookup {
    /// Returns the symbol for `s` if it has already been interned, without
    /// interning it.
    fn get(&self, s: &str) -> Option<Symbol>;

    /// Runs `f` against the string `symbol` names, returning its result, or `None`
    /// if `symbol` is out of range for this interner.
    ///
    /// The borrow passed to `f` is valid only for the duration of the call. This
    /// keeps resolution zero-copy across both the single-threaded and concurrent
    /// interners; the concurrent one holds its read lock for exactly the span of
    /// `f`.
    fn resolve_with<R, F>(&self, symbol: Symbol, f: F) -> Option<R>
    where
        F: FnOnce(&str) -> R;

    /// Returns the number of distinct strings interned so far.
    fn len(&self) -> usize;

    /// Returns `true` if no strings have been interned.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
