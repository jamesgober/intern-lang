# intern-lang &mdash; API Reference

> Complete reference for every public item in `intern-lang`, with examples.
> **Status: stable (1.0).** This surface is frozen under [Semantic Versioning](https://semver.org/) until 2.0 — no breaking change without a major bump. See [Stability](#stability).

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [`Symbol`](#symbol)
- [`Interner`](#interner)
  - [`Interner::new`](#internernew)
  - [`Interner::with_capacity`](#internerwith_capacity)
  - [`Interner::intern`](#internerintern)
  - [`Interner::try_intern`](#internertry_intern)
  - [`Interner::get`](#internerget)
  - [`Interner::resolve`](#internerresolve)
  - [`Interner::resolve_with`](#internerresolve_with)
  - [`Interner::len` / `Interner::is_empty`](#internerlen--interneris_empty)
- [`ConcurrentInterner`](#concurrentinterner)
- [`Lookup`](#lookup)
- [`InternError`](#internerror)
- [Feature flags](#feature-flags)
- [Guarantees](#guarantees)
- [Stability](#stability)

---

## Overview

`intern-lang` maps each distinct string to a small, copyable [`Symbol`](#symbol),
storing the bytes once in a contiguous buffer and handing back an integer handle.
A compiler front-end interns every identifier once, then compares and passes
symbols instead of strings — a name comparison becomes an integer comparison, and
a name in an AST node costs four bytes instead of an owned `String`.

It owns interning only. Lexing is `lexer-lang`; scoping and name resolution are
`symbol-lang`. Keeping this crate to interning is what lets every layer above
share one symbol space cheaply.

The public surface is intentionally small: one handle type and one interner.

---

## Installation

```toml
[dependencies]
intern-lang = "0.4"
```

The crate is `no_std`-compatible (it relies only on `alloc`); the default `std`
feature is additive. [`ConcurrentInterner`](#concurrentinterner) requires the
`std` feature; `serde` support for [`Symbol`](#symbol) is behind the `serde`
feature.

---

## Quick start

```rust
use intern_lang::Interner;

let mut interner = Interner::new();

// Intern returns a small Copy handle; the same string always returns the same one.
let while_kw = interner.intern("while");
let for_kw = interner.intern("for");
assert_eq!(interner.intern("while"), while_kw);

// Name comparisons are now integer comparisons.
assert_ne!(while_kw, for_kw);

// Resolve borrows the original bytes straight out of the store.
assert_eq!(interner.resolve(while_kw), Some("while"));
```

---

## `Symbol`

A small, copyable handle to a string held by an [`Interner`](#interner).

`Symbol` is a newtype over a `NonZeroU32`, so it is four bytes, `Copy`, and as
cheap to pass or store as an integer. Equality, ordering, and hashing are integer
operations and never touch the underlying bytes. The `NonZeroU32` representation
means `Option<Symbol>` is also four bytes, which matters when a symbol is stored
in a large side table.

A `Symbol` is only meaningful together with the interner that issued it. Two
symbols from the *same* interner are equal exactly when they name the same string.
Symbols from different interners share no relationship.

**Derives:** `Clone`, `Copy`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`,
`Debug`.

### `Symbol::as_u32`

```rust
pub fn as_u32(self) -> u32
```

Returns the raw 1-based integer id backing the symbol. The id is assigned in
interning order — the first distinct string interned is `1`, the second `2`, and
so on — and is stable for the lifetime of the issuing interner. It is exposed for
diagnostics, stable ordering, and building external side tables keyed by symbol;
it is not a string and carries no meaning outside its interner.

**Returns:** the `u32` id, always `>= 1`.

```rust
use intern_lang::Interner;

let mut interner = Interner::new();
let first = interner.intern("alpha");
let second = interner.intern("beta");

assert_eq!(first.as_u32(), 1);
assert_eq!(second.as_u32(), 2);

// Re-interning an existing string returns the same id, never a new one.
assert_eq!(interner.intern("alpha").as_u32(), 1);
```

Using a symbol as a map key — equality and hashing stay on the integer:

```rust
use std::collections::HashMap;
use intern_lang::Interner;

let mut interner = Interner::new();
let mut kinds: HashMap<_, &str> = HashMap::new();
kinds.insert(interner.intern("i32"), "integer");
kinds.insert(interner.intern("f64"), "float");

assert_eq!(kinds.get(&interner.intern("i32")), Some(&"integer"));
```

### `Symbol::from_u32`

```rust
pub fn from_u32(id: u32) -> Option<Symbol>
```

Reconstructs a symbol from a raw 1-based id — the inverse of
[`as_u32`](#symbolas_u32) — for rebuilding a symbol from an id stored elsewhere (a
file, a wire message, an external table). Returns `None` for `0`. It only rebuilds
the handle; whether the id names anything is decided when you
[`resolve`](#internerresolve) it, which returns `None` for an out-of-range id.

**Parameters:**

- `id` — a raw symbol id.

**Returns:** `Some(symbol)` for any non-zero `id`, `None` for `0`.

```rust
use intern_lang::{Interner, Symbol};

let mut interner = Interner::new();
let sym = interner.intern("persisted");

let rebuilt = Symbol::from_u32(sym.as_u32()).unwrap();
assert_eq!(rebuilt, sym);
assert_eq!(interner.resolve(rebuilt), Some("persisted"));
assert_eq!(Symbol::from_u32(0), None);
```

---

## `Interner`

The single-threaded string interner.

`Interner` maps each distinct string to a [`Symbol`](#symbol), stores the bytes
exactly once in a contiguous buffer, and hands back integer handles. Interning a
string it has already seen is a hash lookup with no allocation and no copy;
resolving a symbol borrows the original bytes straight out of the buffer.

Bytes live once, appended end to end in a single buffer; a symbol indexes a side
table of `(start, len)` spans into that buffer, so a symbol is four bytes
regardless of string length. Deduplication runs through an open-addressing hash
index that stores symbol ids, not strings, so it adds no second copy of the bytes.
The home slot and the cached fingerprint come from disjoint bits of a fully mixed
64-bit hash, so structured keys — numbered identifiers, long shared prefixes or
suffixes — spread across the index instead of clustering (probe counts are
pinned by tests at 100k keys).

**Derives:** `Debug` (shows the string and byte counts, not the contents),
`Default`.

### `Interner::new`

```rust
pub fn new() -> Self
```

Creates an empty interner. No allocation happens until the first string is
interned, so an interner that is created but never used costs nothing.

```rust
use intern_lang::Interner;

let interner = Interner::new();
assert!(interner.is_empty());
```

### `Interner::with_capacity`

```rust
pub fn with_capacity(capacity: usize) -> Self
```

Creates an empty interner sized to hold about `capacity` distinct strings before
the dedup index has to grow. This pre-allocates the span table and the hash index;
the backing byte buffer is left to grow on demand, since the total byte length
cannot be predicted from a string count.

**Parameters:**

- `capacity` — the expected number of *distinct* strings. `0` behaves like
  [`new`](#internernew).

Use this when the rough number of distinct identifiers is known ahead of time —
for example, sizing from a previous compilation — to avoid a series of
reallocations during warm-up. It changes performance only; results are identical
to an interner built with `new`.

```rust
use intern_lang::Interner;

let mut interner = Interner::with_capacity(4_096);
let sym = interner.intern("identifier");
assert_eq!(interner.resolve(sym), Some("identifier"));
```

### `Interner::intern`

```rust
pub fn intern(&mut self, s: &str) -> Symbol
```

Interns `s` and returns its [`Symbol`](#symbol). If `s` has been interned before,
the existing symbol is returned and nothing is allocated or copied. Otherwise the
bytes are appended to the backing store, a fresh symbol is assigned, and that
symbol is returned.

**Parameters:**

- `s` — the string to intern. Any `&str`, including the empty string and
  arbitrary Unicode, is accepted.

**Returns:** the symbol for `s`. Below the symbol-space bound the result always
round-trips: `interner.resolve(interner.intern(s))` is `Some(s)`.

**At the symbol-space bound** (all `u32::MAX` symbols issued — in practice memory
runs out first): a string that is already interned still returns its own symbol.
A *new* string is not stored, and `intern` returns the highest symbol, id
`u32::MAX`, without panicking. That symbol belongs to the last string interned
before the space filled, so it compares equal to that string's symbol and
`resolve` returns that other string, not `s`; round-trip and distinctness do not
hold for that one return value, and the interner itself stays consistent. No
symbol value can be reserved to mean "no string" without lowering the documented
`u32::MAX` capacity, so 1.x keeps this behaviour (a cleaner contract is planned
for 2.0). Use [`try_intern`](#internertry_intern) wherever the bound could be
approached.

```rust
use intern_lang::Interner;

let mut interner = Interner::new();
let a = interner.intern("while");
let b = interner.intern("while");
let c = interner.intern("until");

assert_eq!(a, b);   // deduplicated — same handle, no new allocation
assert_ne!(a, c);   // distinct strings, distinct symbols
assert_eq!(interner.resolve(a), Some("while"));
```

Folding a token stream down to symbols, so later passes compare integers:

```rust
use intern_lang::{Interner, Symbol};

let mut interner = Interner::new();
let source = ["let", "x", "=", "x", "+", "x"];
let tokens: Vec<Symbol> = source.iter().map(|t| interner.intern(t)).collect();

// The three `x` occurrences collapsed to one symbol.
assert_eq!(tokens[1], tokens[3]);
assert_eq!(tokens[3], tokens[5]);
assert_eq!(interner.len(), 4); // let, x, =, +
```

### `Interner::try_intern`

```rust
pub fn try_intern(&mut self, s: &str) -> Result<Symbol, InternError>
```

The fallible counterpart to [`intern`](#internerintern). It behaves identically —
deduplicating, allocation-free on a repeat hit — except that interning a *new*
string when the symbol space is full returns
[`InternError::SymbolSpaceExhausted`](#internerror) instead of saturating.
Interning a string that already exists never fails.

**Parameters:**

- `s` — the string to intern.

**Returns:** `Ok(symbol)`, or `Err(InternError::SymbolSpaceExhausted)` when `s` is
new and all `u32::MAX` symbols have been issued — unreachable for any input that
fits in memory.

```rust
use intern_lang::Interner;

let mut interner = Interner::new();
let sym = interner.try_intern("identifier").expect("space available");
assert_eq!(interner.resolve(sym), Some("identifier"));

// Re-interning the same string yields the same symbol and never errors.
assert_eq!(interner.try_intern("identifier"), Ok(sym));
```

### `Interner::get`

```rust
pub fn get(&self, s: &str) -> Option<Symbol>
```

Looks up `s` without interning it. Unlike [`intern`](#internerintern), this never
mutates the interner: a miss returns `None` rather than allocating a new symbol.

**Parameters:**

- `s` — the string to look up.

**Returns:** `Some(symbol)` if `s` has already been interned, `None` otherwise.

```rust
use intern_lang::Interner;

let mut interner = Interner::new();
let sym = interner.intern("declared");

assert_eq!(interner.get("declared"), Some(sym));
assert_eq!(interner.get("undeclared"), None);
assert_eq!(interner.len(), 1); // `get` did not add anything
```

### `Interner::resolve`

```rust
pub fn resolve(&self, symbol: Symbol) -> Option<&str>
```

Resolves `symbol` back to the string it names, borrowing the bytes from the
backing store.

**Parameters:**

- `symbol` — a symbol, ideally one this interner issued.

**Returns:** `Some(&str)` for any symbol this interner issued; `None` for a symbol
whose id is out of range — most often one issued by a different interner. A symbol
from another interner whose id happens to fall in range resolves to *this*
interner's string at that id, so symbols should not be resolved across interners.

```rust
use intern_lang::Interner;

let mut interner = Interner::new();
let sym = interner.intern("resolved");
assert_eq!(interner.resolve(sym), Some("resolved"));

// A symbol whose id exceeds this interner's range resolves to None.
let mut other = Interner::new();
let _ = other.intern("a");
let high = other.intern("b");
assert_eq!(interner.resolve(high), None);
```

The borrow is tied to `&self`, so growth never dangles a resolved slice — resolve
again after more interning to get a fresh, valid borrow:

```rust
use intern_lang::Interner;

let mut interner = Interner::new();
let first = interner.intern("first");

// Intern enough to force the backing store to grow.
for i in 0..10_000 {
    let _ = interner.intern(&format!("filler_{i}"));
}

// The early symbol still resolves to its original string.
assert_eq!(interner.resolve(first), Some("first"));
```

### `Interner::resolve_with`

```rust
pub fn resolve_with<R, F>(&self, symbol: Symbol, f: F) -> Option<R>
where
    F: FnOnce(&str) -> R
```

Runs `f` against the string `symbol` names and returns its result, or `None` if
`symbol` is out of range. This is the [`Lookup`](#lookup) trait's resolution form.
For the single-threaded interner it is a thin wrapper over
[`resolve`](#internerresolve) — prefer `resolve`, which hands back the slice
directly. The closure form exists so the same generic code also works against
[`ConcurrentInterner`](#concurrentinterner), where the borrow cannot outlive the
read lock.

**Parameters:**

- `symbol` — a symbol issued by this interner.
- `f` — a closure run against the resolved string.

**Returns:** `Some(f(resolved))`, or `None` if `symbol` is out of range.

```rust
use intern_lang::Interner;

let mut interner = Interner::new();
let sym = interner.intern("identifier");

assert_eq!(interner.resolve_with(sym, str::len), Some(10));
assert_eq!(interner.resolve_with(sym, |s| s.to_uppercase()), Some("IDENTIFIER".to_string()));
```

### `Interner::len` / `Interner::is_empty`

```rust
pub fn len(&self) -> usize
pub fn is_empty(&self) -> bool
```

`len` returns the number of distinct strings interned so far — which is also the
id the next newly interned string will receive. `is_empty` returns `true` when
nothing has been interned.

```rust
use intern_lang::Interner;

let mut interner = Interner::new();
assert_eq!(interner.len(), 0);
assert!(interner.is_empty());

let _ = interner.intern("x");
let _ = interner.intern("x"); // duplicate, not counted again
let _ = interner.intern("y");

assert_eq!(interner.len(), 2);
assert!(!interner.is_empty());
```

---

## `ConcurrentInterner`

A thread-safe interner many threads can intern into at once, sharing one symbol
space. Requires the `std` feature (on by default).

`ConcurrentInterner` wraps [`Interner`](#interner) in an `RwLock` and exposes the
same operations through `&self`. It is additive: storage, deduplication, and the
symbol stability guarantees are exactly those of `Interner`; this type only adds
synchronisation. Interning a string already present is served under a shared read
lock, so the warm-cache path runs concurrently across threads; only a new string
takes the exclusive write lock, and the insert re-checks under it, so two threads
racing to intern the same new string still resolve to one symbol. Lock poisoning
is recovered internally rather than re-raised as a panic.

**Derives:** `Debug` (shows the string count), `Default`. Is `Send + Sync`.

### `ConcurrentInterner::new` / `with_capacity`

```rust
pub fn new() -> Self
pub fn with_capacity(capacity: usize) -> Self
```

Create an empty concurrent interner; `with_capacity` pre-sizes the dedup index for
about `capacity` distinct strings. Both mirror the [`Interner`](#interner)
constructors.

```rust
use intern_lang::ConcurrentInterner;

let interner = ConcurrentInterner::with_capacity(4_096);
assert!(interner.is_empty());
```

### `ConcurrentInterner::intern`

```rust
pub fn intern(&self, s: &str) -> Symbol
```

Interns `s` from a shared `&self` reference. The same string always yields the
same symbol, even when several threads intern it at once.

**Parameters:**

- `s` — the string to intern.

**Returns:** the symbol for `s`. At the symbol-space bound it behaves exactly like
[`Interner::intern`](#internerintern): a new string is not stored and the highest
symbol, which names a different string, is returned without panicking.

```rust
use std::sync::Arc;
use std::thread;

use intern_lang::ConcurrentInterner;

let interner = Arc::new(ConcurrentInterner::new());

let handles: Vec<_> = (0..4)
    .map(|_| {
        let interner = Arc::clone(&interner);
        thread::spawn(move || interner.intern("shared"))
    })
    .collect();

let symbols: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

// All threads agree on one symbol; it was interned exactly once.
assert!(symbols.iter().all(|&s| s == symbols[0]));
assert_eq!(interner.len(), 1);
```

### `ConcurrentInterner::try_intern`

```rust
pub fn try_intern(&self, s: &str) -> Result<Symbol, InternError>
```

The fallible counterpart to [`intern`](#concurrentinternerintern), with the same
two-step locking. A string already present is returned under the read lock and
never errors; only a new string at the symbol-space bound returns
[`InternError::SymbolSpaceExhausted`](#internerror).

```rust
use intern_lang::ConcurrentInterner;

let interner = ConcurrentInterner::new();
let sym = interner.try_intern("name").expect("space available");
assert_eq!(interner.try_intern("name"), Ok(sym));
```

### `ConcurrentInterner::get`

```rust
pub fn get(&self, s: &str) -> Option<Symbol>
```

Read-only lookup that never interns. Returns `Some(symbol)` if `s` has been
interned, `None` otherwise.

```rust
use intern_lang::ConcurrentInterner;

let interner = ConcurrentInterner::new();
let sym = interner.intern("present");
assert_eq!(interner.get("present"), Some(sym));
assert_eq!(interner.get("absent"), None);
```

### `ConcurrentInterner::resolve_with`

```rust
pub fn resolve_with<R, F>(&self, symbol: Symbol, f: F) -> Option<R>
where
    F: FnOnce(&str) -> R
```

Runs `f` against the resolved string while holding the read lock, returning its
result, or `None` if `symbol` is out of range. This is the zero-copy resolution
path — keep `f` short, since it runs under the lock.

```rust
use intern_lang::ConcurrentInterner;

let interner = ConcurrentInterner::new();
let sym = interner.intern("measured");
assert_eq!(interner.resolve_with(sym, str::len), Some(8));
```

### `ConcurrentInterner::resolve`

```rust
pub fn resolve(&self, symbol: Symbol) -> Option<String>
```

Resolves `symbol` to an owned `String`, copying the bytes out so the result
outlives the lock. On a hot path that only inspects the string, prefer
[`resolve_with`](#concurrentinternerresolve_with) to avoid the allocation.

```rust
use intern_lang::ConcurrentInterner;

let interner = ConcurrentInterner::new();
let sym = interner.intern("owned");
assert_eq!(interner.resolve(sym).as_deref(), Some("owned"));
```

### `ConcurrentInterner::len` / `is_empty`

```rust
pub fn len(&self) -> usize
pub fn is_empty(&self) -> bool
```

Report the number of distinct strings interned, and whether that is zero.

---

## `Lookup`

The read-side contract both interners implement, so generic code can accept
either the single-threaded [`Interner`](#interner) or the
[`ConcurrentInterner`](#concurrentinterner).

```rust
pub trait Lookup {
    fn get(&self, s: &str) -> Option<Symbol>;
    fn resolve_with<R, F>(&self, symbol: Symbol, f: F) -> Option<R>
    where
        F: FnOnce(&str) -> R;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool { self.len() == 0 } // provided
}
```

Interning is deliberately not on the trait: the two interners disagree on how it
is called (`&mut self` for the single-threaded one, `&self` for the concurrent
one), and forcing a common signature would tax the single-threaded hot path with
synchronisation it does not need. Resolution is expressed as `resolve_with` rather
than a method returning `&str`, because a concurrent interner can only expose the
bytes while it holds its read lock.

The trait is not object-safe (`resolve_with` is generic), so use it as a generic
bound (`&impl Lookup` / `T: Lookup`), not as `dyn Lookup`.

```rust
use intern_lang::{ConcurrentInterner, Interner, Lookup};

// Works against either interner kind.
fn name_len<L: Lookup>(interner: &L, s: &str) -> Option<usize> {
    let sym = interner.get(s)?;
    interner.resolve_with(sym, str::len)
}

let mut single = Interner::new();
let _ = single.intern("alpha");
assert_eq!(name_len(&single, "alpha"), Some(5));

let shared = ConcurrentInterner::new();
let _ = shared.intern("beta");
assert_eq!(name_len(&shared, "beta"), Some(4));
```

---

## `InternError`

The error returned by [`try_intern`](#internertry_intern). It is
`#[non_exhaustive]` and implements `core::error::Error` (zero dependencies,
`no_std`).

```rust
#[non_exhaustive]
pub enum InternError {
    /// The symbol space is exhausted: all `u32::MAX` symbols have been issued.
    SymbolSpaceExhausted,
}
```

Because the enum is `#[non_exhaustive]`, a `match` on it must include a wildcard
arm — future releases may add variants without it being a breaking change.

```rust
use intern_lang::{InternError, Interner};

let mut interner = Interner::new();
assert!(interner.try_intern("name").is_ok());

fn describe(err: InternError) -> &'static str {
    match err {
        InternError::SymbolSpaceExhausted => "out of symbols",
        _ => "unknown interning error",
    }
}
assert_eq!(describe(InternError::SymbolSpaceExhausted), "out of symbols");
```

---

## Feature flags

| Feature | Default | Description |
|---------|---------|-------------|
| `std` | yes | Use the standard library and enable [`ConcurrentInterner`](#concurrentinterner). With it disabled the crate is `no_std` (it always relies on `alloc`) and only the single-threaded [`Interner`](#interner) is available. |
| `serde` | no | Serialise/deserialise [`Symbol`](#symbol) (transparently, as its integer id). |

`intern-lang` has no runtime dependencies beyond an optional `serde`.

---

## Guarantees

These hold for every symbol an `Interner` issues, and are covered by property
tests against a `HashMap` reference interner:

- **Dedup.** Interning the same string twice returns the same `Symbol`.
- **Distinctness.** Interning two distinct strings returns two distinct symbols.
- **Round-trip.** `resolve(intern(s))` is `Some(s)` for every accepted `s`.
- **Stability.** A `Symbol` keeps resolving to the same string for the whole
  lifetime of its interner, including after the backing store grows.
- **Cost.** `Symbol` is `Copy` and four bytes; passing one is never more expensive
  than passing an integer.

Symbol ids span `1..=u32::MAX`, so an interner holds up to `u32::MAX` distinct
strings — a bound that exhausts memory long before the id space. At the bound,
[`try_intern`](#internertry_intern) returns
[`InternError::SymbolSpaceExhausted`](#internerror), while
[`intern`](#internerintern) saturates: it returns the highest symbol, which names
the last string interned rather than the new one, so round-trip and distinctness
hold only below the bound for `intern` (see its entry for the exact behaviour).

---

## Stability

**As of 1.0.0 this surface is stable and frozen under
[Semantic Versioning](https://semver.org/) until 2.0.** Every item documented
above — its name, signature, and documented behaviour — is part of the contract:
no breaking change will be made without a major version bump. Patch and minor
releases may fix bugs, improve performance, sharpen documentation, and *add* new
items, but will never remove or alter an existing one.

Two deliberate extension points keep the freeze from precluding growth:

- `InternError` is `#[non_exhaustive]`, so new failure modes can be added as
  variants without a breaking change. Always include a wildcard `match` arm.
- New methods, feature flags, and trait implementations may still be *added* under
  SemVer's additive rule; nothing in this reference will be taken away or altered.

The MSRV is **1.85**. An MSRV increase is treated as a minor, not a patch, change.

---

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>
