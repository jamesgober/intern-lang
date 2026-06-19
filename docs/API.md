# intern-lang &mdash; API Reference

> Complete reference for every public item in `intern-lang`, with examples.
> **Status: pre-1.0 — the surface below is the planned design and is being built across the 0.x series.** Items marked _(planned)_ are not yet implemented; see [`dev/ROADMAP.md`](../dev/ROADMAP.md).

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [`Symbol`](#symbol) _(planned, v0.2.0)_
- [`Interner`](#interner) _(planned, v0.2.0)_
- [`ConcurrentInterner`](#concurrentinterner) _(planned, v0.3.0)_
- [Feature flags](#feature-flags)

---

## Overview

intern-lang maps each distinct string to a small, copyable [`Symbol`](#symbol),
storing the bytes once and handing back an integer handle. A compiler front-end
interns every identifier once, then compares and passes symbols instead of
strings — a name comparison becomes an integer comparison, and a name in an AST
node costs four bytes instead of an owned `String`.

It owns interning only. Lexing is `lexer-lang`; scoping and name resolution are
`symbol-lang`. Keeping this crate to interning is what lets every layer above
share one symbol space cheaply.

---

## Installation

```toml
[dependencies]
intern-lang = "0.1"
```

The crate is `no_std`-compatible (it relies on `alloc`); the default `std`
feature is additive.

---

## `Symbol`

_(planned, v0.2.0)_ A small `Copy` handle to an interned string — a newtype over
a 32-bit id. Equality and hashing are integer operations.

```rust,ignore
use intern_lang::Interner;

let mut interner = Interner::new();
let a = interner.intern("loop");
let b = interner.intern("loop");
assert_eq!(a, b);                       // same string -> same symbol
assert_eq!(interner.resolve(a), "loop"); // round-trips
```

## `Interner`

_(planned, v0.2.0)_ The single-threaded interner. `intern(&str) -> Symbol`
deduplicates and returns a stable handle; `resolve(Symbol) -> &str` borrows the
original bytes back out of the contiguous store. Symbols stay valid for the
interner's lifetime, including after the store grows.

## `ConcurrentInterner`

_(planned, v0.3.0)_ A thread-safe interner many front-end threads can intern into
at once, sharing one symbol space, behind the same trait seam as `Interner`.

---

## Feature flags

| Feature | Default | Description |
|---------|---------|-------------|
| `std` | yes | Use the standard library. With it disabled the crate is `no_std` (it always relies on `alloc`). |
| `serde` | no | Serialise/deserialise `Symbol`. |

intern-lang has no runtime dependencies beyond an optional `serde`.

---

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>
