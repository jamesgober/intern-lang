# intern-lang &mdash; API Reference

> Complete reference for every public item in `intern-lang`, with examples.
> **Status: pre-1.0 — the surface is being designed across the 0.x series and frozen at `1.0.0`.** Items marked _(planned)_ are not yet implemented; see [`dev/ROADMAP.md`](../dev/ROADMAP.md).

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [Quick start](#quick-start)
- [`Symbol`](#symbol)
- [`Interner`](#interner)
  - [`Interner::new`](#internernew)
  - [`Interner::with_capacity`](#internerwith_capacity)
  - [`Interner::intern`](#internerintern)
  - [`Interner::get`](#internerget)
  - [`Interner::resolve`](#internerresolve)
  - [`Interner::len` / `Interner::is_empty`](#internerlen--interneris_empty)
- [`ConcurrentInterner`](#concurrentinterner) _(planned, v0.3.0)_
- [Feature flags](#feature-flags)
- [Guarantees](#guarantees)

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
intern-lang = "0.2"
```

The crate is `no_std`-compatible (it relies only on `alloc`); the default `std`
feature is additive.

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

**Returns:** the symbol for `s`. The result always round-trips:
`interner.resolve(interner.intern(s))` is `Some(s)`.

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

_(planned, v0.3.0)_ A thread-safe interner many front-end threads can intern into
at once, sharing one symbol space, behind the same trait seam as `Interner`.

---

## Feature flags

| Feature | Default | Description |
|---------|---------|-------------|
| `std` | yes | Use the standard library. With it disabled the crate is `no_std` (it always relies on `alloc`). |
| `serde` | no | Serialise/deserialise `Symbol`. _(planned, v0.4.0)_ |

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
strings — a bound that exhausts memory long before the id space. A defined,
non-panicking exhaustion result is scheduled for v0.4.0.

---

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>
