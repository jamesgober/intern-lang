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
/// no second copy of the bytes. Its home slots come from a fully mixed hash, so
/// structured keys — numbered identifiers, long shared prefixes or suffixes —
/// spread across the index instead of piling onto a few slots.
///
/// The buffer only ever appends and the span table only ever grows, so a symbol
/// issued early keeps resolving to the same string for the interner's whole
/// lifetime, including after either structure reallocates —
/// [`resolve`](Interner::resolve) recomputes the slice from the current buffer on
/// each call rather than holding a borrowed pointer, so growth can never dangle a
/// previously issued symbol.
///
/// # Capacity
///
/// Symbol ids span `1..=u32::MAX`, so an interner holds up to `u32::MAX` distinct
/// strings. Reaching that bound requires interning over four billion *distinct*
/// strings: the span table alone would need about 64 GiB and the dedup index tens
/// of gigabytes more, so in practice memory runs out first. The bound is still
/// defined and non-panicking: [`try_intern`](Interner::try_intern) reports it as
/// [`InternError::SymbolSpaceExhausted`], and [`intern`](Interner::intern)
/// saturates exactly as its "Symbol-space bound" section describes.
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
    /// approached. Exactly what it does once all `u32::MAX` symbols are issued:
    ///
    /// - A string that is **already interned** still returns its own symbol, as
    ///   always.
    /// - A **new** string is *not* stored, and `intern` returns the highest
    ///   symbol, id `u32::MAX`, without panicking. That symbol already belongs to
    ///   the last string interned before the space filled, so the returned value
    ///   compares equal to that other string's symbol and
    ///   [`resolve`](Interner::resolve) yields that other string, not `s`. The
    ///   round-trip and distinctness guarantees do not hold for that one return
    ///   value; the interner itself is unchanged and stays consistent.
    ///
    /// No symbol value can be reserved to mean "no string" without lowering the
    /// documented `u32::MAX` capacity, so 1.x keeps this saturating behaviour; a
    /// cleaner contract is planned for 2.0. If a workload can plausibly approach
    /// the bound, call [`try_intern`](Interner::try_intern), which reports it as
    /// [`InternError::SymbolSpaceExhausted`] instead.
    pub fn intern(&mut self, s: &str) -> Symbol {
        let hash = hash_bytes(s.as_bytes());
        if let Some(symbol) = self.lookup(s, hash) {
            return symbol;
        }
        if self.is_full() {
            // Symbol space exhausted: saturate at the highest symbol rather than
            // panic. That id names the last string interned, not `s`; the doc
            // comment above states this exactly, and `try_intern` is the
            // reporting path. Memory runs out before this in practice (the bound
            // is `u32::MAX`), but the behaviour is defined and tested.
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
        let fp = fingerprint(hash);
        let mut idx = slot_index(hash, self.mask);
        loop {
            let slot = self.table[idx];
            if slot.is_empty() {
                return None;
            }
            if slot.hash == fp && self.span_bytes(slot.id) == s.as_bytes() {
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
        self.insert_slot(hash, id);
        Symbol::from_raw(id)
    }

    /// Records symbol `id` (whose string hashed to `hash`) at its first empty
    /// probe position. The table is guaranteed to have room because
    /// [`reserve_one`](Interner::reserve_one) ran first.
    fn insert_slot(&mut self, hash: u64, id: u32) {
        let mut idx = slot_index(hash, self.mask);
        while !self.table[idx].is_empty() {
            idx = (idx + 1) & self.mask;
        }
        self.table[idx] = Slot {
            hash: fingerprint(hash),
            id,
        };
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
            let mut idx = slot_index(hash, mask);
            while !table[idx].is_empty() {
                idx = (idx + 1) & mask;
            }
            table[idx] = Slot {
                hash: fingerprint(hash),
                id,
            };
        }

        self.table = table;
        self.mask = mask;
    }

    /// Returns the stored bytes for a 1-based symbol id. Only called with ids the
    /// interner issued, so the span always exists. The probe compares bytes, not
    /// `&str`, so it skips the char-boundary checks a `str` slice would repeat on
    /// every candidate; equal bytes are equal strings.
    #[inline]
    fn span_bytes(&self, id: u32) -> &[u8] {
        let span = self.spans[id as usize - 1];
        &self.buf.as_bytes()[span.start..span.start + span.len]
    }

    /// Test-only probe counter: the number of slots [`lookup`](Interner::lookup)
    /// inspects to find `s`, or to prove it absent. It re-walks the probe sequence
    /// instead of instrumenting the real one, so the hot path carries no counter.
    ///
    /// For a present string this equals the probes its placement in the *current*
    /// table cost: the index never deletes, and a resize re-inserts every key in
    /// id order, so nothing moves a key after it is placed.
    #[cfg(test)]
    fn probe_count(&self, s: &str) -> usize {
        if self.table.is_empty() {
            return 0;
        }
        let hash = hash_bytes(s.as_bytes());
        let mut idx = slot_index(hash, self.mask);
        let mut probes = 1;
        loop {
            let slot = self.table[idx];
            if slot.is_empty()
                || (slot.hash == fingerprint(hash) && self.span_bytes(slot.id) == s.as_bytes())
            {
                return probes;
            }
            idx = (idx + 1) & self.mask;
            probes += 1;
        }
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

/// Maps a hash to its home slot in a table of `mask + 1` slots.
///
/// Uses the low bits. That is only sound because [`hash_bytes`] ends in a
/// full-width avalanche ([`finish`]): every low bit depends on every input byte
/// (see H03 there).
#[inline]
fn slot_index(hash: u64, mask: usize) -> usize {
    (hash as usize) & mask
}

/// The 32-bit fingerprint cached in a [`Slot`] to reject non-matches without
/// touching the backing buffer.
///
/// Taken from the *high* half, which the slot index (the low bits, for any table
/// under 2^32 slots) does not read directly: two keys that share a home slot
/// still almost never share a fingerprint, so a probe through a collision run
/// rarely falls through to a byte comparison.
#[inline]
fn fingerprint(hash: u64) -> u32 {
    (hash >> 32) as u32
}

/// Hashes `bytes` to a well-mixed 64-bit value for the dedup index.
///
/// The body is an FxHash-style multiply-rotate over 64-bit little-endian words,
/// seeded with the length. Strings of up to eight bytes are packed into a single
/// word with overlapping fixed-width reads (two `u32`s for 4..=8 bytes, three
/// single bytes for 1..=3), and a longer string's partial last word is read as
/// the *final eight bytes* of the string, overlapping the previous word. Every
/// byte reaches the state through a fixed-size load, so no length-dependent copy
/// sits on the hot path, and for a given length the packing is injective. Each
/// round is a bijection of the state for a fixed word, so two same-length strings
/// that differ in a single word never collide on the full 64-bit value.
///
/// The body alone is not fit for slot selection (H03, fixed in 1.0.1): a
/// multiply carries information only *upward*, so the low `k` bits of the result
/// depend only on the low `k` bits of the last word, which for a short string are
/// its first two or three bytes and its length. Numbered identifiers such as
/// `t0000..t7999` or `identifier_number_{i}` then share a handful of home slots
/// and linear probing degrades to quadratic total work. [`finish`] closes this
/// with a xor-shift / multiply / xor-shift avalanche that makes the low bits
/// depend on the whole state.
///
/// This is a non-cryptographic, unkeyed hash chosen for throughput on short
/// identifiers. Correctness never depends on it: the dedup index always confirms
/// a candidate with a full byte comparison.
#[inline]
fn hash_bytes(bytes: &[u8]) -> u64 {
    let len = bytes.len();
    let mut hash = len as u64;
    if len <= 8 {
        if len > 0 {
            hash = round(hash, small_word(bytes));
        }
    } else {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in chunks.by_ref() {
            hash = round(hash, read_u64(chunk));
        }
        if !chunks.remainder().is_empty() {
            // Re-read the last eight bytes rather than copying a short tail into
            // a zeroed buffer. `len > 8`, so the subtraction cannot underflow.
            hash = round(hash, read_u64(&bytes[len - 8..]));
        }
    }
    finish(hash)
}

/// One FxHash-style round: fold `word` into `state`.
#[inline]
fn round(state: u64, word: u64) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    (state.rotate_left(5) ^ word).wrapping_mul(K)
}

/// Packs a string of 1..=8 bytes into one word using fixed-width loads only.
///
/// For 4..=8 bytes the first and last four bytes are read (overlapping when the
/// length is under eight); for 1..=3 bytes the first, middle, and last byte are
/// taken. Either way every byte lands in the word, so for a fixed length the
/// packing is injective — the length itself is already in the hash state.
#[inline]
fn small_word(bytes: &[u8]) -> u64 {
    let len = bytes.len();
    if len >= 4 {
        let lo = read_u32(&bytes[..4]);
        let hi = read_u32(&bytes[len - 4..]);
        u64::from(lo) | (u64::from(hi) << 32)
    } else {
        u64::from(bytes[0]) | (u64::from(bytes[len / 2]) << 8) | (u64::from(bytes[len - 1]) << 16)
    }
}

/// Reads the first eight bytes of `bytes` (which holds at least eight) as a
/// little-endian word.
#[inline]
fn read_u64(bytes: &[u8]) -> u64 {
    let mut word = [0u8; 8];
    word.copy_from_slice(&bytes[..8]);
    u64::from_le_bytes(word)
}

/// Reads the first four bytes of `bytes` (which holds at least four) as a
/// little-endian word.
#[inline]
fn read_u32(bytes: &[u8]) -> u32 {
    let mut word = [0u8; 4];
    word.copy_from_slice(&bytes[..4]);
    u32::from_le_bytes(word)
}

/// Finalizer: xor-shift, multiply, xor-shift.
///
/// The first shift folds the high half of the state onto the low half, so the
/// multiply's low bits already see every state bit; the multiply then spreads
/// each bit upward, and the last shift brings the well-mixed high bits back down
/// into the low bits the slot index uses. Three cheap 64-bit operations, once per
/// string rather than per word, and no 128-bit arithmetic (so 32-bit targets pay
/// no widening-multiply penalty). The multiplier is 2^64 / φ, the usual
/// Fibonacci-hashing constant (odd, so the multiply is a bijection).
#[inline]
fn finish(state: u64) -> u64 {
    const M: u64 = 0x9e37_79b9_7f4a_7c15;
    let h = (state ^ (state >> 32)).wrapping_mul(M);
    h ^ (h >> 32)
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
        // and is not stored. Pin the documented (M29) consequence exactly: that
        // symbol is the last-interned string's, so it names "b", not "c".
        let saturated = interner.intern("c");
        assert_eq!(saturated.as_u32(), 2);
        assert_eq!(saturated, b);
        assert_eq!(interner.resolve(saturated), Some("b"));
        assert_eq!(interner.get("c"), None);
        assert_eq!(interner.len(), 2);
        // Existing strings still resolve and dedup correctly.
        assert_eq!(interner.resolve(a), Some("a"));
        assert_eq!(interner.intern("b"), b);
    }

    // ---- Probe distribution (H03) --------------------------------------------
    //
    // These pin the quality of slot selection, not just correctness: a hash that
    // dedups correctly can still pile structured keys onto a handful of home slots
    // and turn every insert into a long linear walk. Linear probing at the 0.75
    // load ceiling averages 2.5 probes per present key in theory, so the bounds
    // below leave headroom for an honest hash while sitting orders of magnitude
    // under the clustered behaviour they guard against (thousands of probes per
    // key on the 1.0.0 hash).

    /// Average probes a present key may cost.
    const MAX_AVG_HIT_PROBES: f64 = 3.0;
    /// Average probes an absent key may cost (theory: 8.5 at the 0.75 ceiling).
    const MAX_AVG_MISS_PROBES: f64 = 10.0;
    /// Worst single probe sequence for any key in any corpus.
    const MAX_WORST_PROBES: usize = 128;

    struct ProbeStats {
        avg_hit: f64,
        worst_hit: usize,
        avg_miss: f64,
        worst_miss: usize,
    }

    /// Interns `strings` into a fresh interner and measures the probe cost of
    /// finding each one again, plus the cost of a miss for a sibling key that is
    /// guaranteed absent (each string with a byte appended that no corpus uses).
    fn probe_stats(strings: &[String]) -> ProbeStats {
        let mut interner = Interner::new();
        for s in strings {
            let _ = interner.intern(s);
        }
        assert_eq!(interner.len(), strings.len(), "corpus must be distinct");

        let (mut hit_total, mut worst_hit) = (0usize, 0usize);
        let (mut miss_total, mut worst_miss) = (0usize, 0usize);
        let mut absent = String::new();
        for s in strings {
            let hit = interner.probe_count(s);
            hit_total += hit;
            worst_hit = worst_hit.max(hit);

            absent.clear();
            absent.push_str(s);
            absent.push('\u{1}');
            assert_eq!(interner.get(&absent), None);
            let miss = interner.probe_count(&absent);
            miss_total += miss;
            worst_miss = worst_miss.max(miss);
        }
        let n = strings.len() as f64;
        ProbeStats {
            avg_hit: hit_total as f64 / n,
            worst_hit,
            avg_miss: miss_total as f64 / n,
            worst_miss,
        }
    }

    fn assert_well_distributed(name: &str, strings: &[String]) {
        let stats = probe_stats(strings);
        assert!(
            stats.avg_hit <= MAX_AVG_HIT_PROBES,
            "{name}: average hit probes {:.2} > {MAX_AVG_HIT_PROBES}",
            stats.avg_hit
        );
        assert!(
            stats.avg_miss <= MAX_AVG_MISS_PROBES,
            "{name}: average miss probes {:.2} > {MAX_AVG_MISS_PROBES}",
            stats.avg_miss
        );
        assert!(
            stats.worst_hit <= MAX_WORST_PROBES && stats.worst_miss <= MAX_WORST_PROBES,
            "{name}: worst probe sequence {} hit / {} miss > {MAX_WORST_PROBES}",
            stats.worst_hit,
            stats.worst_miss
        );
    }

    fn numbered(prefix: &str, n: usize) -> Vec<String> {
        (0..n).map(|i| alloc::format!("{prefix}{i}")).collect()
    }

    /// The exact shape reported in H03: short numbered identifiers
    /// `t0000..t7999`, which the 1.0.0 hash folded onto one home slot per
    /// length class.
    #[test]
    fn h03_numbered_identifiers_do_not_cluster() {
        let ids: Vec<String> = (0..8_000).map(|i| alloc::format!("t{i:04}")).collect();
        assert_well_distributed("t0000..t7999", &ids);
        let vars: Vec<String> = (0..8_000).map(|i| alloc::format!("var{i:04}")).collect();
        assert_well_distributed("var0000..var7999", &vars);
    }

    #[test]
    fn probes_bounded_for_100k_numbered_ids() {
        assert_well_distributed("t{i}", &numbered("t", 100_000));
        let padded: Vec<String> = (0..100_000).map(|i| alloc::format!("v{i:06}")).collect();
        assert_well_distributed("v{i:06}", &padded);
    }

    #[test]
    fn probes_bounded_for_common_prefix_ids() {
        assert_well_distributed(
            "common prefix",
            &numbered("very::long::common::module::path::item_", 100_000),
        );
    }

    #[test]
    fn probes_bounded_for_common_suffix_ids() {
        let ids: Vec<String> = (0..100_000)
            .map(|i| alloc::format!("{i}_with_a_long_common_suffix_tail"))
            .collect();
        assert_well_distributed("common suffix", &ids);
    }

    /// The benchmark suite's own corpus, which the 1.0.0 hash clustered too.
    #[test]
    fn probes_bounded_for_bench_corpus() {
        assert_well_distributed(
            "identifier_number_{i}",
            &numbered("identifier_number_", 100_000),
        );
    }

    #[test]
    fn probes_bounded_for_single_byte_strings() {
        let ids: Vec<String> = (0u8..=0x7f).map(|b| String::from(char::from(b))).collect();
        assert_well_distributed("single byte", &ids);
        // Every two-byte ASCII string: the other packing path for tiny keys.
        let pairs: Vec<String> = (0u8..=0x7f)
            .flat_map(|a| (0u8..=0x7f).map(move |b| [char::from(a), char::from(b)]))
            .map(|pair| pair.iter().collect())
            .collect();
        assert_well_distributed("two byte", &pairs);
    }

    /// The word packing must carry every byte: for each length up to three
    /// words, changing any single byte must change the full 64-bit hash. This
    /// pins the overlapping fixed-width reads (short-string packing and the
    /// re-read final word) against an off-by-one that silently drops a byte.
    #[test]
    fn hash_sees_every_byte_at_every_length() {
        for len in 0..=24usize {
            let base = alloc::vec![b'a'; len];
            let base_hash = hash_bytes(&base);
            for pos in 0..len {
                let mut changed = base.clone();
                changed[pos] = b'b';
                assert_ne!(
                    hash_bytes(&changed),
                    base_hash,
                    "len {len}: byte {pos} does not reach the hash"
                );
            }
        }
    }

    /// No two keys of these structured corpora share a full 64-bit hash.
    #[test]
    fn hash_has_no_full_width_collisions_on_structured_corpora() {
        let mut corpora = alloc::vec![
            numbered("t", 100_000),
            numbered("identifier_number_", 100_000),
            numbered("very::long::common::module::path::item_", 100_000),
        ];
        corpora.push((0u8..=0x7f).map(|b| String::from(char::from(b))).collect());
        for corpus in &corpora {
            let mut seen = alloc::collections::BTreeSet::new();
            for s in corpus {
                assert!(seen.insert(hash_bytes(s.as_bytes())), "collision on {s:?}");
            }
        }
    }

    #[test]
    fn probes_bounded_for_long_strings() {
        // 1 KiB strings that share almost every byte and differ at the start, in
        // the middle, or at the end — the hash must carry every word through.
        let filler = "x".repeat(1_000);
        let mut ids = Vec::new();
        for i in 0..4_000 {
            ids.push(alloc::format!("{i:06}{filler}"));
            ids.push(alloc::format!("{filler}{i:06}"));
            ids.push(alloc::format!("{}{i:06}{}", &filler[..500], &filler[500..]));
        }
        assert_well_distributed("long strings", &ids);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// H03 generalised: any `{prefix}{counter}{suffix}` family — the shape of
        /// generated names, temporaries, and mangled paths — spreads across the
        /// index, whatever the shared prefix and suffix are.
        #[test]
        fn numbered_families_stay_well_distributed(
            prefix in "[a-zA-Z_:]{0,40}",
            suffix in "[a-zA-Z_]{0,24}",
            zero_pad in proptest::bool::ANY,
        ) {
            let ids: Vec<String> = (0..5_000)
                .map(|i| if zero_pad {
                    alloc::format!("{prefix}{i:05}{suffix}")
                } else {
                    alloc::format!("{prefix}{i}{suffix}")
                })
                .collect();
            let stats = probe_stats(&ids);
            prop_assert!(stats.avg_hit <= MAX_AVG_HIT_PROBES, "avg hit {:.2}", stats.avg_hit);
            prop_assert!(stats.avg_miss <= MAX_AVG_MISS_PROBES, "avg miss {:.2}", stats.avg_miss);
            prop_assert!(
                stats.worst_hit <= MAX_WORST_PROBES && stats.worst_miss <= MAX_WORST_PROBES,
                "worst {} / {}", stats.worst_hit, stats.worst_miss
            );
        }
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
