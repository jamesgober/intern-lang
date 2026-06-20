//! The single-threaded [`Interner`]: a contiguous string store with a
//! deduplicating index.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::error::InternError;
use crate::symbol::Symbol;

/// Initial number of slots in the dedup index. A power of two so the hash maps to
/// a slot with a single mask. Sixteen keeps an empty interner cheap while still
/// avoiding an immediate resize for small inputs.
const INITIAL_CAPACITY: usize = 16;

/// Where one interned string lives inside the backing buffer. `start` and `len`
/// are byte offsets into [`Interner::buf`]; the pair is the symbol's permanent
/// coordinates, and because the buffer only ever appends, they never change once
/// assigned — that is what keeps a symbol resolving to the same bytes after the
/// store grows.
#[derive(Clone, Copy)]
struct Span {
    start: usize,
    len: usize,
}

/// One slot in the open-addressing dedup index. `id` is the 1-based symbol id, or
/// `0` for an empty slot (symbol ids start at one, so zero is a free sentinel).
/// `hash` caches the low 32 bits of the string's hash so a probe can reject a
/// non-match without touching the backing buffer at all.
#[derive(Clone, Copy)]
struct Slot {
    hash: u32,
    id: u32,
}

impl Slot {
    const EMPTY: Slot = Slot { hash: 0, id: 0 };

    #[inline]
    fn is_empty(self) -> bool {
        self.id == 0
    }
}

/// A single-threaded string interner.
///
/// `Interner` maps each distinct string to a small [`Symbol`], stores the bytes
/// exactly once in a contiguous buffer, and hands back integer handles. Interning
/// a string it has already seen is a hash lookup with no allocation and no copy;
/// resolving a symbol borrows the original bytes straight out of the buffer.
///
/// # Design
///
/// Bytes live once, appended end to end in a single `String`. A symbol is an
/// index into a side table of `(start, len)` spans into that buffer, so a symbol
/// is four bytes regardless of how long its string is. Deduplication runs through
/// an open-addressing hash index that stores symbol ids, not strings, so it adds
/// no second copy of the bytes. The buffer only ever appends and the span table
/// only ever grows, so a symbol issued early keeps resolving to the same string
/// for the interner's whole lifetime, including after either structure
/// reallocates — [`resolve`](Interner::resolve) recomputes the slice from the
/// current buffer on each call rather than holding a borrowed pointer, so growth
/// can never dangle a previously issued symbol.
///
/// # Capacity
///
/// Symbol ids span `1..=u32::MAX`, so an interner holds up to `u32::MAX` distinct
/// strings. Reaching that bound requires interning over four billion *distinct*
/// strings, which exhausts memory long before the id space — the span table alone
/// would need tens of gigabytes. A defined, non-panicking exhaustion result is
/// scheduled for a later release; until then the boundary is unreachable for any
/// input that fits in memory.
///
/// # Examples
///
/// ```
/// use intern_lang::Interner;
///
/// let mut interner = Interner::new();
///
/// let print = interner.intern("print");
/// let again = interner.intern("print");
/// let read = interner.intern("read");
///
/// // Deduplication: the same string always yields the same symbol.
/// assert_eq!(print, again);
/// assert_ne!(print, read);
///
/// // Resolution borrows the stored bytes back out.
/// assert_eq!(interner.resolve(print), Some("print"));
/// assert_eq!(interner.len(), 2);
/// ```
pub struct Interner {
    /// Contiguous backing store. Every interned string's bytes are appended here
    /// once and never moved relative to their span.
    buf: String,
    /// Span per symbol, indexed by the symbol's 0-based [`Symbol::index`]. Push
    /// order is interning order, so `spans.len()` is also the next 1-based id.
    spans: Vec<Span>,
    /// Open-addressing dedup index. Length is a power of two; `mask` is
    /// `len - 1`. Empty until the first insert.
    table: Vec<Slot>,
    /// `table.len() - 1`, for mapping a hash to a slot with a single `&`.
    mask: usize,
    /// Size of the symbol space: the most distinct strings this interner will
    /// issue symbols for. Always `u32::MAX` in normal use — the constructors set
    /// it there and nothing lowers it — so it is unreachable before memory runs
    /// out. It exists so [`try_intern`](Interner::try_intern) has a defined,
    /// testable exhaustion boundary.
    max_symbols: u32,
}

impl Interner {
    /// Creates an empty interner.
    ///
    /// No allocation happens until the first string is interned, so an interner
    /// that is created but never used costs nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let interner = Interner::new();
    /// assert!(interner.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            spans: Vec::new(),
            table: Vec::new(),
            mask: 0,
            max_symbols: u32::MAX,
        }
    }

    /// Creates an empty interner sized to hold about `capacity` distinct strings
    /// before the dedup index has to grow.
    ///
    /// This pre-allocates the span table and the hash index. The backing byte
    /// buffer is left to grow on demand, since the total byte length cannot be
    /// predicted from a string count. Use this when the rough number of distinct
    /// identifiers is known ahead of time — for example, sizing from a previous
    /// compilation — to avoid a series of reallocations during warm-up.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let mut interner = Interner::with_capacity(1_024);
    /// let sym = interner.intern("identifier");
    /// assert_eq!(interner.resolve(sym), Some("identifier"));
    /// ```
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let mut interner = Self::new();
        if capacity > 0 {
            interner.spans.reserve(capacity);
            let table_cap = table_capacity_for(capacity);
            interner.resize_table(table_cap);
        }
        interner
    }

    /// Interns `s`, returning its [`Symbol`].
    ///
    /// If `s` has been interned before, the existing symbol is returned and
    /// nothing is allocated or copied. Otherwise the bytes are appended to the
    /// backing store, a fresh symbol is assigned, and that symbol is returned.
    /// Either way the result round-trips: `interner.resolve(interner.intern(s))`
    /// is always `Some(s)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let mut interner = Interner::new();
    /// let a = interner.intern("while");
    /// let b = interner.intern("while");
    /// let c = interner.intern("until");
    ///
    /// assert_eq!(a, b);            // deduplicated
    /// assert_ne!(a, c);            // distinct strings, distinct symbols
    /// assert_eq!(interner.resolve(a), Some("while"));
    /// ```
    ///
    /// # Symbol-space bound
    ///
    /// An interner issues at most `u32::MAX` distinct symbols. `intern` is the
    /// infallible path for the overwhelming common case where that bound is never
    /// approached; at the bound it saturates — returning the highest symbol
    /// without adding the string — rather than panicking. Use
    /// [`try_intern`](Interner::try_intern) when you need the exhaustion reported
    /// as a [`Result`] instead.
    pub fn intern(&mut self, s: &str) -> Symbol {
        let hash = hash_bytes(s.as_bytes());
        if let Some(symbol) = self.lookup(s, hash) {
            return symbol;
        }
        if self.is_full() {
            // Symbol space exhausted: saturate at the highest symbol rather than
            // panic. Unreachable in normal use (the bound is `u32::MAX`).
            return Symbol::from_raw(self.max_symbols);
        }
        self.insert_new(s, hash)
    }

    /// Interns `s`, returning its [`Symbol`], or an error if the symbol space is
    /// exhausted.
    ///
    /// This is the fallible counterpart to [`intern`](Interner::intern). It
    /// behaves identically — deduplicating, allocation-free on a repeat hit —
    /// except that interning a *new* string when the symbol space is full returns
    /// [`InternError::SymbolSpaceExhausted`] instead of saturating. Interning a
    /// string that already exists never fails, even at the bound.
    ///
    /// # Errors
    ///
    /// Returns [`InternError::SymbolSpaceExhausted`] when `s` is new and the
    /// interner has already issued all of its symbols. This is unreachable for any
    /// input that fits in memory; the method exists so a caller that must account
    /// for the boundary can do so explicitly.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let mut interner = Interner::new();
    /// let sym = interner.try_intern("identifier").expect("space available");
    /// assert_eq!(interner.resolve(sym), Some("identifier"));
    ///
    /// // Re-interning the same string yields the same symbol and never errors.
    /// assert_eq!(interner.try_intern("identifier"), Ok(sym));
    /// ```
    pub fn try_intern(&mut self, s: &str) -> Result<Symbol, InternError> {
        let hash = hash_bytes(s.as_bytes());
        if let Some(symbol) = self.lookup(s, hash) {
            return Ok(symbol);
        }
        if self.is_full() {
            return Err(InternError::SymbolSpaceExhausted);
        }
        Ok(self.insert_new(s, hash))
    }

    /// Whether the symbol space is exhausted — no new string can be assigned a
    /// symbol.
    #[inline]
    fn is_full(&self) -> bool {
        self.spans.len() >= self.max_symbols as usize
    }

    /// Looks up `s` without interning it, returning its [`Symbol`] if it is
    /// already present.
    ///
    /// Unlike [`intern`](Interner::intern), this never mutates the interner: a
    /// miss returns `None` rather than allocating a new symbol. Use it to ask
    /// "has this name been seen?" without growing the symbol space.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let mut interner = Interner::new();
    /// let sym = interner.intern("declared");
    ///
    /// assert_eq!(interner.get("declared"), Some(sym));
    /// assert_eq!(interner.get("undeclared"), None);
    /// ```
    #[must_use]
    pub fn get(&self, s: &str) -> Option<Symbol> {
        self.lookup(s, hash_bytes(s.as_bytes()))
    }

    /// Resolves `symbol` back to the string it names, borrowing the bytes from the
    /// backing store.
    ///
    /// Returns `Some(&str)` for any symbol this interner issued, and `None` for a
    /// symbol whose id is out of range — most often one issued by a different
    /// interner. A symbol from another interner whose id happens to fall in range
    /// resolves to *this* interner's string at that id; symbols are only
    /// meaningful with the interner that produced them.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let mut interner = Interner::new();
    /// let sym = interner.intern("resolved");
    /// assert_eq!(interner.resolve(sym), Some("resolved"));
    ///
    /// // A symbol from an interner that issued more symbols is out of range here.
    /// let mut other = Interner::new();
    /// let _ = other.intern("a");
    /// let high = other.intern("b");
    /// assert_eq!(interner.resolve(high), None);
    /// ```
    #[must_use]
    pub fn resolve(&self, symbol: Symbol) -> Option<&str> {
        let span = self.spans.get(symbol.index())?;
        Some(&self.buf[span.start..span.start + span.len])
    }

    /// Runs `f` against the string `symbol` names, returning its result, or `None`
    /// if `symbol` is out of range.
    ///
    /// This is the [`Lookup`](crate::Lookup) trait's resolution form. For the
    /// single-threaded interner it is a thin wrapper over
    /// [`resolve`](Interner::resolve) — prefer `resolve` here, which hands back the
    /// borrowed slice directly. The closure form exists so the same generic code
    /// works against the [`ConcurrentInterner`](crate::ConcurrentInterner), where
    /// the borrow cannot outlive the read lock.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let mut interner = Interner::new();
    /// let sym = interner.intern("identifier");
    /// assert_eq!(interner.resolve_with(sym, str::len), Some(10));
    /// ```
    pub fn resolve_with<R, F>(&self, symbol: Symbol, f: F) -> Option<R>
    where
        F: FnOnce(&str) -> R,
    {
        self.resolve(symbol).map(f)
    }

    /// Returns the number of distinct strings interned so far.
    ///
    /// This is also the id that the next newly interned string will receive.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let mut interner = Interner::new();
    /// assert_eq!(interner.len(), 0);
    /// let _ = interner.intern("x");
    /// let _ = interner.intern("x"); // duplicate, not counted again
    /// let _ = interner.intern("y");
    /// assert_eq!(interner.len(), 2);
    /// ```
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.spans.len()
    }

    /// Returns `true` if no strings have been interned.
    ///
    /// # Examples
    ///
    /// ```
    /// use intern_lang::Interner;
    ///
    /// let mut interner = Interner::new();
    /// assert!(interner.is_empty());
    /// let _ = interner.intern("x");
    /// assert!(!interner.is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Probes the dedup index for `s`. Returns its symbol if present.
    fn lookup(&self, s: &str, hash: u64) -> Option<Symbol> {
        if self.table.is_empty() {
            return None;
        }
        let fingerprint = hash as u32;
        let mut idx = (hash as usize) & self.mask;
        loop {
            let slot = self.table[idx];
            if slot.is_empty() {
                return None;
            }
            if slot.hash == fingerprint && self.span_str(slot.id) == s {
                return Some(Symbol::from_raw(slot.id));
            }
            idx = (idx + 1) & self.mask;
        }
    }

    /// Appends `s` to the backing store, assigns it a fresh symbol, and records it
    /// in the dedup index. The caller has already established that `s` is not
    /// present and computed its `hash`.
    fn insert_new(&mut self, s: &str, hash: u64) -> Symbol {
        self.reserve_one();

        let span = Span {
            start: self.buf.len(),
            len: s.len(),
        };
        self.buf.push_str(s);
        self.spans.push(span);

        // The 1-based id equals the new length of the span table.
        let id = id_for(self.spans.len());
        self.insert_slot(Slot {
            hash: hash as u32,
            id,
        });
        Symbol::from_raw(id)
    }

    /// Places `slot` at its first empty probe position. The table is guaranteed to
    /// have room because [`reserve_one`](Interner::reserve_one) ran first.
    fn insert_slot(&mut self, slot: Slot) {
        let mut idx = (slot.hash as usize) & self.mask;
        while !self.table[idx].is_empty() {
            idx = (idx + 1) & self.mask;
        }
        self.table[idx] = slot;
    }

    /// Ensures the dedup index has room for one more entry under a 0.75 load
    /// factor, allocating or doubling the table as needed.
    fn reserve_one(&mut self) {
        let occupied_after = self.spans.len() + 1;
        if self.table.is_empty() {
            self.resize_table(INITIAL_CAPACITY);
        } else if occupied_after * 4 > self.table.len() * 3 {
            self.resize_table(self.table.len() * 2);
        }
    }

    /// Reallocates the dedup index to `new_cap` slots (a power of two) and
    /// re-inserts every existing symbol. The backing buffer and span table are
    /// untouched, so no symbol changes identity.
    fn resize_table(&mut self, new_cap: usize) {
        let mut table = Vec::new();
        table.resize(new_cap, Slot::EMPTY);
        let mask = new_cap - 1;

        for (i, span) in self.spans.iter().enumerate() {
            let s = &self.buf[span.start..span.start + span.len];
            let hash = hash_bytes(s.as_bytes());
            let id = id_for(i + 1);
            let mut idx = (hash as usize) & mask;
            while !table[idx].is_empty() {
                idx = (idx + 1) & mask;
            }
            table[idx] = Slot {
                hash: hash as u32,
                id,
            };
        }

        self.table = table;
        self.mask = mask;
    }

    /// Returns the string for a 1-based symbol id. Only called with ids the
    /// interner issued, so the span always exists.
    #[inline]
    fn span_str(&self, id: u32) -> &str {
        let span = self.spans[id as usize - 1];
        &self.buf[span.start..span.start + span.len]
    }
}

impl Default for Interner {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl crate::Lookup for Interner {
    #[inline]
    fn get(&self, s: &str) -> Option<Symbol> {
        Interner::get(self, s)
    }

    #[inline]
    fn resolve_with<R, F>(&self, symbol: Symbol, f: F) -> Option<R>
    where
        F: FnOnce(&str) -> R,
    {
        Interner::resolve_with(self, symbol, f)
    }

    #[inline]
    fn len(&self) -> usize {
        Interner::len(self)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        Interner::is_empty(self)
    }
}

impl fmt::Debug for Interner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Interner")
            .field("strings", &self.spans.len())
            .field("bytes", &self.buf.len())
            .finish_non_exhaustive()
    }
}

/// Converts a span-table length into a 1-based symbol id.
///
/// `len` is bounded by available memory — each interned string costs a span, a
/// table slot, and at least one byte — so it stays within `u32` long before the
/// cast could lose information. The saturating fallback keeps the conversion free
/// of `unwrap`/`expect` and panics, and is unreachable for any in-memory input.
#[inline]
fn id_for(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// Rounds a desired distinct-string count up to a power-of-two table capacity that
/// holds it under a 0.75 load factor, never below [`INITIAL_CAPACITY`].
#[inline]
fn table_capacity_for(strings: usize) -> usize {
    let target = strings.saturating_mul(4) / 3 + 1;
    target.max(INITIAL_CAPACITY).next_power_of_two()
}

/// Hashes `bytes` with an FxHash-style multiply-rotate over 64-bit words.
///
/// The string length seeds the state so that strings differing only in trailing
/// content within a word boundary (for example `"ab"` versus `"ab\0"`) do not
/// collide on the fast fingerprint. This is a non-cryptographic hash chosen for
/// throughput on short identifiers; correctness never depends on it, since the
/// dedup index always confirms a candidate with a full byte comparison.
#[inline]
fn hash_bytes(bytes: &[u8]) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;

    let mut hash = bytes.len() as u64;
    let mut chunks = bytes.chunks_exact(8);
    for chunk in chunks.by_ref() {
        // `chunks_exact(8)` always yields eight bytes, so the conversion holds;
        // the fallback is dead and only keeps this free of `unwrap`.
        let word = u64::from_le_bytes(<[u8; 8]>::try_from(chunk).unwrap_or([0; 8]));
        hash = (hash.rotate_left(5) ^ word).wrapping_mul(K);
    }

    let remainder = chunks.remainder();
    if !remainder.is_empty() {
        let mut tail = [0u8; 8];
        tail[..remainder.len()].copy_from_slice(remainder);
        let word = u64::from_le_bytes(tail);
        hash = (hash.rotate_left(5) ^ word).wrapping_mul(K);
    }

    hash
}

#[cfg(test)]
mod tests {
    // Unwrapping is acceptable in tests where an error cannot be meaningfully
    // handled and a failure should fail the test.
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use proptest::prelude::*;

    use super::*;

    #[test]
    fn test_intern_same_string_returns_same_symbol() {
        let mut interner = Interner::new();
        let a = interner.intern("name");
        let b = interner.intern("name");
        assert_eq!(a, b);
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn test_intern_distinct_strings_return_distinct_symbols() {
        let mut interner = Interner::new();
        let a = interner.intern("one");
        let b = interner.intern("two");
        assert_ne!(a, b);
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn test_resolve_roundtrips() {
        let mut interner = Interner::new();
        for s in ["", "a", "alpha", "a longer identifier with spaces"] {
            let sym = interner.intern(s);
            assert_eq!(interner.resolve(sym), Some(s));
        }
    }

    #[test]
    fn test_resolve_out_of_range_symbol_is_none() {
        let mut issuer = Interner::new();
        let _ = issuer.intern("a");
        let high = issuer.intern("b");

        let empty = Interner::new();
        assert_eq!(empty.resolve(high), None);
    }

    #[test]
    fn test_get_does_not_intern() {
        let mut interner = Interner::new();
        assert_eq!(interner.get("absent"), None);
        assert_eq!(interner.len(), 0);
        let sym = interner.intern("absent");
        assert_eq!(interner.get("absent"), Some(sym));
    }

    #[test]
    fn test_ids_are_sequential_from_one() {
        let mut interner = Interner::new();
        assert_eq!(interner.intern("a").as_u32(), 1);
        assert_eq!(interner.intern("b").as_u32(), 2);
        assert_eq!(interner.intern("a").as_u32(), 1);
        assert_eq!(interner.intern("c").as_u32(), 3);
    }

    #[test]
    fn test_growth_preserves_earlier_symbols() {
        let mut interner = Interner::new();
        let mut remembered = alloc::vec::Vec::new();
        // Enough distinct strings to force several table resizes and buffer
        // reallocations.
        for i in 0..10_000 {
            let s = alloc::format!("symbol_{i}");
            remembered.push((interner.intern(&s), s));
        }
        for (sym, s) in &remembered {
            assert_eq!(interner.resolve(*sym), Some(s.as_str()));
        }
    }

    #[test]
    fn test_empty_string_is_interned() {
        let mut interner = Interner::new();
        let empty = interner.intern("");
        assert_eq!(interner.resolve(empty), Some(""));
        assert_eq!(interner.intern(""), empty);
    }

    #[test]
    fn test_unicode_roundtrips() {
        let mut interner = Interner::new();
        for s in ["café", "naïve", "日本語", "emoji 🦀", "Ωμέγα"] {
            let sym = interner.intern(s);
            assert_eq!(interner.resolve(sym), Some(s));
        }
    }

    #[test]
    fn test_with_capacity_behaves_like_new() {
        let mut interner = Interner::with_capacity(64);
        let sym = interner.intern("preallocated");
        assert_eq!(interner.resolve(sym), Some("preallocated"));
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn test_strings_differing_only_in_trailing_byte_are_distinct() {
        let mut interner = Interner::new();
        let a = interner.intern("ab");
        let b = interner.intern("ab\0");
        assert_ne!(a, b);
        assert_eq!(interner.resolve(a), Some("ab"));
        assert_eq!(interner.resolve(b), Some("ab\0"));
    }

    #[test]
    fn test_default_is_empty() {
        let interner = Interner::default();
        assert!(interner.is_empty());
    }

    #[test]
    fn test_table_capacity_for_is_power_of_two_and_fits() {
        for n in [0usize, 1, 12, 13, 100, 1000] {
            let cap = table_capacity_for(n);
            assert!(cap.is_power_of_two());
            assert!(cap >= INITIAL_CAPACITY);
            assert!(cap * 3 >= n.saturating_mul(4));
        }
    }

    #[test]
    fn test_try_intern_succeeds_below_the_bound() {
        let mut interner = Interner::new();
        let sym = interner.try_intern("ok").expect("space available");
        assert_eq!(interner.resolve(sym), Some("ok"));
        assert_eq!(interner.try_intern("ok"), Ok(sym));
    }

    #[test]
    fn test_intern_saturates_at_the_bound() {
        // Lower the symbol-space bound so the boundary is reachable in a test.
        let mut interner = Interner::new();
        interner.max_symbols = 2;
        let a = interner.intern("a");
        let b = interner.intern("b");
        // The space is now full; a new string saturates to the highest symbol
        // and is not stored.
        let saturated = interner.intern("c");
        assert_eq!(saturated.as_u32(), 2);
        assert_eq!(interner.len(), 2);
        // Existing strings still resolve and dedup correctly.
        assert_eq!(interner.resolve(a), Some("a"));
        assert_eq!(interner.intern("b"), b);
    }

    proptest! {
        /// At the symbol-space boundary, `try_intern` reports exhaustion for a new
        /// string while still accepting strings it already holds.
        #[test]
        fn try_intern_reports_exhaustion_at_the_boundary(limit in 1u32..=64) {
            let mut interner = Interner::new();
            interner.max_symbols = limit;

            // Fill exactly to the bound with distinct strings.
            for i in 0..limit {
                let s = alloc::format!("s{i}");
                prop_assert!(interner.try_intern(&s).is_ok());
            }
            prop_assert_eq!(interner.len(), limit as usize);

            // A new distinct string is now rejected with the defined error...
            prop_assert_eq!(
                interner.try_intern("overflow"),
                Err(InternError::SymbolSpaceExhausted)
            );
            // ...but an already-interned string still succeeds (dedup, no growth).
            prop_assert!(interner.try_intern("s0").is_ok());
            prop_assert_eq!(interner.len(), limit as usize);
        }
    }
}
