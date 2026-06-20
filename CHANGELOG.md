<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>intern-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

### Added

### Changed

### Fixed

### Security

---

## [0.4.0] - 2026-06-20

Serde, the exhaustion contract, and the feature freeze. This release completes the
public surface: a defined non-panicking error for symbol-space exhaustion,
optional `serde` for `Symbol`, and a declaration that the API is now frozen ahead
of 1.0. No breaking changes — everything here is additive.

### Added

- `InternError` — a typed, `#[non_exhaustive]` error implementing
  `core::error::Error` (zero dependencies, `no_std`). Its `SymbolSpaceExhausted`
  variant is the defined outcome when the symbol space is full.
- `Interner::try_intern` and `ConcurrentInterner::try_intern` — the fallible
  interning path: identical to `intern` but returning
  `Err(InternError::SymbolSpaceExhausted)` for a new string at the symbol-space
  bound rather than saturating. Property-tested at the boundary.
- `Symbol::from_u32` — reconstruct a symbol from a raw id (the inverse of
  `as_u32`), returning `None` for `0`.
- `serde` support for `Symbol` behind the `serde` feature: it serializes
  transparently as its integer id and deserializes back, rejecting `0`.
  Round-trip property-tested.

### Changed

- `Interner::intern` and `ConcurrentInterner::intern` now document their behaviour
  at the symbol-space bound: they saturate at the highest symbol rather than
  panic. Behaviour below the bound — every realistic workload — is unchanged.
- The public API is declared **frozen**: `docs/API.md` records the complete 1.0
  surface and the SemVer promise. No public API will be added or changed before
  1.0.0, which will mark it stable.

---

## [0.3.0] - 2026-06-20

The concurrent interner. `ConcurrentInterner` lets many threads intern into one
shared symbol space at once, behind the same read-side seam as the single-threaded
`Interner`. It is additive: the storage, the dedup index, and the symbol stability
guarantees are unchanged — this release only adds synchronisation, and the
single-threaded hot path is untouched.

### Added

- `ConcurrentInterner` (requires the `std` feature): a thread-safe interner with
  `new`, `with_capacity`, `intern(&self, &str)`, `get`, `resolve_with`,
  `resolve` (owned), `len`, `is_empty`, `Default`, and `Debug`. `Send + Sync`.
  - Interning a string already present is served under a shared read lock, so the
    warm-cache path runs concurrently; only a new string takes the exclusive write
    lock, and the insert re-checks under it so racing threads never mint a
    duplicate symbol for the same string.
  - Lock poisoning is recovered rather than propagated as a second panic.
- `Lookup` trait: the read-side contract (`get`, `resolve_with`, `len`,
  `is_empty`) implemented by both `Interner` and `ConcurrentInterner`, so generic
  code can accept either.
- `Interner::resolve_with` — closure-based resolution mirroring the trait form.
- Multi-threaded contention tests proving no duplicate symbols across threads, a
  lock-poison recovery test, and `Send + Sync` assertions.
- A concurrent benchmark group measuring warm-path throughput at 1/4/8 threads.

---

## [0.2.0] - 2026-06-19

The core interner and symbol. `Interner` interns a string to a small `Copy`
`Symbol`, stores the bytes once in a contiguous buffer, and resolves a symbol
back to a borrowed slice. The hard part of the design — symbol stability across
backing-store growth — is implemented and property-tested here, against a
`HashMap` reference interner.

### Added

- `Symbol`: a four-byte `Copy` handle over a `NonZeroU32`, with integer equality,
  ordering, and hashing. `as_u32` exposes the raw 1-based id; `Option<Symbol>` is
  niche-optimised to four bytes.
- `Interner`: the single-threaded interner.
  - `new`, `with_capacity`, and `Default`.
  - `intern(&str) -> Symbol` — deduplicating, allocation-free on a repeat hit.
  - `get(&str) -> Option<Symbol>` — read-only lookup that never interns.
  - `resolve(Symbol) -> Option<&str>` — borrows the stored bytes; `None` for a
    symbol whose id is out of range.
  - `len`, `is_empty`.
- Contiguous backing store with an open-addressing dedup index that stores symbol
  ids rather than a second copy of the bytes.
- Property tests covering dedup, distinctness, round-trip fidelity, `get`/`intern`
  agreement, and symbol stability across store growth.
- Integration tests for edge cases: empty strings, Unicode, very long strings,
  foreign/out-of-range symbols, high-volume interning, and `Symbol` map keys.
- Criterion benchmarks for the intern (hit/miss), resolve, and lookup-miss paths.

### Changed

- Hardened the crate-root lint set to the project standard (`missing_docs`,
  `unused_must_use`, `unused_results`, and the `clippy` panic/print/todo denies);
  `unsafe` remains forbidden — the contiguous store is implemented without it.
- Aligned `clippy.toml` `msrv` with the declared `rust-version` (`1.85`),
  removing the MSRV-mismatch warning.
- Corrected the `deny.toml` header that referenced another crate.

---

## [0.1.0] - 2026-06-18

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).

[Unreleased]: https://github.com/jamesgober/intern-lang/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/jamesgober/intern-lang/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/jamesgober/intern-lang/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jamesgober/intern-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/intern-lang/releases/tag/v0.1.0
