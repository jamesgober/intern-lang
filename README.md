<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>intern-lang</b>
    <br>
    <sub><sup>STRING & SYMBOL INTERNING</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/intern-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/intern-lang"></a>
    <a href="https://crates.io/crates/intern-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/intern-lang?color=%230099ff"></a>
    <a href="https://docs.rs/intern-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/intern-lang"></a>
    <a href="https://github.com/jamesgober/intern-lang/actions"><img alt="CI" src="https://github.com/jamesgober/intern-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        intern-lang turns repeated identifiers into small copyable symbols backed by a single contiguous store, so a compiler front-end compares names by integer equality instead of walking bytes. It is the deduplication layer a lexer and symbol table lean on: intern once, then pass cheap Copy handles everywhere a string name would otherwise travel.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition).
    </p>
    <blockquote>
        <strong>Status: pre-1.0, in active development.</strong> The public API is being designed across the 0.x series and frozen at <code>1.0.0</code>. See <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

## Installation

```toml
[dependencies]
intern-lang = "0.2"
```

<br>

## Quick start

```rust
use intern_lang::Interner;

let mut interner = Interner::new();

// Intern returns a small Copy handle; the same string always returns the same one.
let while_kw = interner.intern("while");
let for_kw = interner.intern("for");
assert_eq!(interner.intern("while"), while_kw);

// Name comparisons are now integer comparisons, not byte walks.
assert_ne!(while_kw, for_kw);

// Resolve borrows the original bytes straight out of the store.
assert_eq!(interner.resolve(while_kw), Some("while"));
```

Folding a token stream down to symbols, so later passes compare integers and a
name in an AST node costs four bytes instead of an owned `String`:

```rust
use intern_lang::{Interner, Symbol};

let mut interner = Interner::new();
let source = ["let", "x", "=", "x", "+", "x"];
let tokens: Vec<Symbol> = source.iter().map(|t| interner.intern(t)).collect();

// The three `x` occurrences collapsed to one symbol.
assert_eq!(tokens[1], tokens[3]);
assert_eq!(interner.len(), 4); // let, x, =, +
```

<br>

## Features

- **Four-byte handle.** A `Symbol` is a `NonZeroU32` newtype — `Copy`, integer
  equality / ordering / hashing, and `Option<Symbol>` is four bytes too.
- **Bytes stored once.** Interned strings are appended end to end in a single
  contiguous buffer; the dedup index stores symbol ids, not a second copy.
- **Allocation-free hits.** Interning a string already seen is a hash lookup with
  no allocation and no copy.
- **Growth-stable symbols.** A symbol keeps resolving to the same string for the
  interner's whole lifetime, including after the backing store reallocates.
- **`no_std`.** Relies only on `alloc`; the default `std` feature is additive. No
  runtime dependencies beyond an optional `serde` (planned).
- **`#![forbid(unsafe_code)]`.** The contiguous store is implemented without any
  `unsafe`.

<br>

## Performance

Resolution is a side-table index plus a slice — measured at roughly **1.1 ns per
`resolve`** (Criterion mean, Windows x86_64, Rust stable, on the development
machine). Interning an already-seen string is allocation-free: a hash over the
bytes, an open-addressing probe, and one byte comparison to confirm the hit. The
numbers below are a v0.x baseline, tracked over time rather than advertised as
final:

| Operation | Mean |
|---|---|
| `resolve` (id → `&str`) | ~1.1 ns |
| `intern` (repeat hit, no allocation) | ~0.23 µs |
| `intern` (new string, amortised growth) | ~0.62 µs |

Run them yourself with `cargo bench`.

<br>

## API overview

For the complete reference with examples, see [`docs/API.md`](./docs/API.md).

- [`Symbol`](./docs/API.md#symbol) — four-byte `Copy` handle to an interned string.
- [`Interner`](./docs/API.md#interner) — single-threaded interner: `intern`,
  `get`, `resolve`, `len`, `with_capacity`.

<br>

## Status

`v0.2.0` ships the core interner and symbol. The thread-safe `ConcurrentInterner`,
optional `serde` for `Symbol`, and the defined symbol-space-exhaustion result land
across the remaining 0.x series per the <a href="./dev/ROADMAP.md"><code>ROADMAP</code></a>; the public surface freezes at <code>1.0.0</code>.

<hr>
<br>

## Contributing

See <a href="./dev/DIRECTIVES.md"><code>dev/DIRECTIVES.md</code></a> for engineering standards and the definition of done. Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober <me@jamesgober.com>.</strong></sup>
</div>
