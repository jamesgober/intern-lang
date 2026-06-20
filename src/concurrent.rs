//! The thread-safe [`ConcurrentInterner`].

use alloc::string::{String, ToString};
use core::fmt;
use std::sync::{PoisonError, RwLock};

use crate::error::InternError;
use crate::interner::Interner;
use crate::lookup::Lookup;
use crate::symbol::Symbol;

/// A thread-safe string interner that many threads can intern into at once.
///
/// `ConcurrentInterner` wraps the single-threaded [`Interner`] in an `RwLock` and
/// exposes the same operations through `&self`, so a pool of lexer or parser
/// threads can share one symbol space. It is additive, not a rewrite: the storage,
/// the dedup index, and the symbol stability guarantees are exactly those of
/// [`Interner`]; this type only adds the synchronisation.
///
/// # Correctness under contention
///
/// Interning takes a two-step path. A string that is already present is found
/// under a *shared read lock*, so the common warm-cache case — most identifiers
/// have been seen before — runs concurrently across threads with no exclusive
/// access. Only a genuinely new string escalates to the *exclusive write lock*,
/// and the insert re-checks for the string while holding it, so two threads
/// racing to intern the same new string still resolve to one symbol: the writer
/// that loses the race finds the winner's entry instead of creating a duplicate.
/// The `RwLock`'s exclusivity is what makes "no duplicate symbols for the same
/// string" hold without a custom lock-free protocol.
///
/// Reads dominate once warm, so the `RwLock` is the right primitive here; writes
/// to new strings serialise. If write contention on a write-heavy workload ever
/// shows up in benchmarks, the storage can be sharded behind this same surface
/// without changing the API.
///
/// # Lock poisoning
///
/// If a thread panics while holding the lock it becomes poisoned. The interner's
/// own operations do not panic, so a poisoned lock means a panic originated
/// elsewhere; rather than propagate that as a second panic, every method recovers
/// the guard and continues. The stored data is structurally intact because
/// interning is the only mutator and it does not unwind partway through under any
/// input that fits in memory.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use std::thread;
///
/// use intern_lang::ConcurrentInterner;
///
/// let interner = Arc::new(ConcurrentInterner::new());
///
/// let mut handles = Vec::new();
/// for _ in 0..4 {
///     let interner = Arc::clone(&interner);
///     handles.push(thread::spawn(move || {
///         // Every thread interns the same names; they all agree on the symbols.
///         (interner.intern("loop"), interner.intern("break"))
///     }));
/// }
///
/// let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
/// let first = results[0];
/// assert!(results.iter().all(|&pair| pair == first));
/// assert_eq!(interner.len(), 2); // "loop" and "break", interned once each
/// ```
#[cfg_attr(docsrs, doc(cfg(feature = "std")))]
pub struct ConcurrentInterner {
    inner: RwLock<Interner>,
}

impl ConcurrentInterner {
    /// Creates an empty concurrent interner. Like [`Interner::new`], no allocation
    /// happens until the first string is interned.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::ConcurrentInterner;
    ///
    /// let interner = ConcurrentInterner::new();
    /// assert!(interner.is_empty());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Interner::new()),
        }
    }

    /// Creates an empty concurrent interner sized for about `capacity` distinct
    /// strings before the dedup index grows. See [`Interner::with_capacity`].
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::ConcurrentInterner;
    ///
    /// let interner = ConcurrentInterner::with_capacity(4_096);
    /// assert!(interner.is_empty());
    /// ```
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: RwLock::new(Interner::with_capacity(capacity)),
        }
    }

    /// Interns `s` from a shared reference, returning its [`Symbol`].
    ///
    /// A string already present is resolved under a shared read lock; only a new
    /// string takes the exclusive write lock. The same string always yields the
    /// same symbol, even when several threads intern it at once.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::ConcurrentInterner;
    ///
    /// let interner = ConcurrentInterner::new();
    /// let a = interner.intern("shared");
    /// let b = interner.intern("shared");
    /// assert_eq!(a, b);
    /// ```
    pub fn intern(&self, s: &str) -> Symbol {
        // Fast path: a string that already exists is found under a shared lock,
        // so concurrent readers do not block one another.
        {
            let guard = self.inner.read().unwrap_or_else(PoisonError::into_inner);
            if let Some(symbol) = guard.get(s) {
                return symbol;
            }
        }
        // Slow path: take the exclusive lock and intern. `Interner::intern`
        // re-checks for the string, so a racer that inserted it between our read
        // and write returns the existing symbol rather than a duplicate.
        let mut guard = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        guard.intern(s)
    }

    /// Interns `s` from a shared reference, returning its [`Symbol`], or an error
    /// if the symbol space is exhausted.
    ///
    /// The fallible counterpart to [`intern`](Self::intern), with the same
    /// two-step locking. A string already present is returned under the read lock
    /// and never errors; only a new string at the symbol-space bound returns
    /// [`InternError::SymbolSpaceExhausted`].
    ///
    /// # Errors
    ///
    /// Returns [`InternError::SymbolSpaceExhausted`] when `s` is new and the
    /// interner has issued all of its symbols — unreachable for any input that
    /// fits in memory.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::ConcurrentInterner;
    ///
    /// let interner = ConcurrentInterner::new();
    /// let sym = interner.try_intern("name").expect("space available");
    /// assert_eq!(interner.try_intern("name"), Ok(sym));
    /// ```
    pub fn try_intern(&self, s: &str) -> Result<Symbol, InternError> {
        {
            let guard = self.inner.read().unwrap_or_else(PoisonError::into_inner);
            if let Some(symbol) = guard.get(s) {
                return Ok(symbol);
            }
        }
        let mut guard = self.inner.write().unwrap_or_else(PoisonError::into_inner);
        guard.try_intern(s)
    }

    /// Returns the symbol for `s` if it has already been interned, without
    /// interning it.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::ConcurrentInterner;
    ///
    /// let interner = ConcurrentInterner::new();
    /// let sym = interner.intern("present");
    /// assert_eq!(interner.get("present"), Some(sym));
    /// assert_eq!(interner.get("absent"), None);
    /// ```
    #[must_use]
    pub fn get(&self, s: &str) -> Option<Symbol> {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(s)
    }

    /// Runs `f` against the string `symbol` names, returning its result, or `None`
    /// if `symbol` is out of range.
    ///
    /// The read lock is held only for the duration of `f`, so the borrow stays
    /// zero-copy without escaping the lock. Keep `f` short; it runs under the lock.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::ConcurrentInterner;
    ///
    /// let interner = ConcurrentInterner::new();
    /// let sym = interner.intern("measured");
    /// assert_eq!(interner.resolve_with(sym, str::len), Some(8));
    /// ```
    pub fn resolve_with<R, F>(&self, symbol: Symbol, f: F) -> Option<R>
    where
        F: FnOnce(&str) -> R,
    {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .resolve(symbol)
            .map(f)
    }

    /// Resolves `symbol` to an owned `String`, or `None` if it is out of range.
    ///
    /// This is the ergonomic counterpart to [`resolve_with`](Self::resolve_with):
    /// it copies the bytes out so the result outlives the lock. On a hot path that
    /// only inspects the string, prefer `resolve_with` to avoid the allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::ConcurrentInterner;
    ///
    /// let interner = ConcurrentInterner::new();
    /// let sym = interner.intern("owned");
    /// assert_eq!(interner.resolve(sym).as_deref(), Some("owned"));
    /// ```
    #[must_use]
    pub fn resolve(&self, symbol: Symbol) -> Option<String> {
        self.resolve_with(symbol, ToString::to_string)
    }

    /// Returns the number of distinct strings interned so far.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::ConcurrentInterner;
    ///
    /// let interner = ConcurrentInterner::new();
    /// let _ = interner.intern("a");
    /// let _ = interner.intern("a");
    /// let _ = interner.intern("b");
    /// assert_eq!(interner.len(), 2);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Returns `true` if no strings have been interned.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::ConcurrentInterner;
    ///
    /// let interner = ConcurrentInterner::new();
    /// assert!(interner.is_empty());
    /// let _ = interner.intern("x");
    /// assert!(!interner.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for ConcurrentInterner {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Lookup for ConcurrentInterner {
    #[inline]
    fn get(&self, s: &str) -> Option<Symbol> {
        ConcurrentInterner::get(self, s)
    }

    #[inline]
    fn resolve_with<R, F>(&self, symbol: Symbol, f: F) -> Option<R>
    where
        F: FnOnce(&str) -> R,
    {
        ConcurrentInterner::resolve_with(self, symbol, f)
    }

    #[inline]
    fn len(&self) -> usize {
        ConcurrentInterner::len(self)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        ConcurrentInterner::is_empty(self)
    }
}

impl fmt::Debug for ConcurrentInterner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConcurrentInterner")
            .field("strings", &self.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_deduplicates() {
        let interner = ConcurrentInterner::new();
        let a = interner.intern("x");
        let b = interner.intern("x");
        assert_eq!(a, b);
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn test_resolve_roundtrips() {
        let interner = ConcurrentInterner::new();
        let sym = interner.intern("value");
        assert_eq!(interner.resolve(sym).as_deref(), Some("value"));
        assert_eq!(interner.resolve_with(sym, str::len), Some(5));
    }

    #[test]
    fn test_get_does_not_intern() {
        let interner = ConcurrentInterner::new();
        assert_eq!(interner.get("absent"), None);
        assert!(interner.is_empty());
    }

    #[test]
    fn test_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ConcurrentInterner>();
    }

    #[test]
    fn test_recovers_from_poisoned_lock() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        use std::sync::Arc;

        let interner = Arc::new(ConcurrentInterner::new());
        let first = interner.intern("before");

        // Poison the lock by panicking while holding the write guard.
        let poisoner = Arc::clone(&interner);
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = poisoner
                .inner
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            panic!("poison the lock");
        }));

        // The interner still works and earlier symbols still resolve.
        assert_eq!(interner.resolve(first).as_deref(), Some("before"));
        let second = interner.intern("after");
        assert_eq!(interner.resolve(second).as_deref(), Some("after"));
    }
}
