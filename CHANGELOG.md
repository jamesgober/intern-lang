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

## [1.0.1] - 2026-10-08

A patch release on the frozen 1.0 surface: no public item is added, removed, or
changed. It fixes a hash-index clustering defect that made structured keys
quadratic to intern, and corrects the symbol-space-bound documentation. See the
[release notes](docs/release/v1.0.1.md) for the full before/after benchmark
numbers.

### Fixed

- **Structured keys no longer cluster in the dedup index (ledger H03).** The
  1.0.0 hash was an FxHash-style multiply with no finalizer, and the home slot
  was taken from its low bits. A multiply carries information only upward, so
  those bits depended on just the first two or three bytes and the length of a
  short string. Numbered identifiers (`t0000..t7999`, `var0000..`), long shared
  prefixes, and the crate's own benchmark corpus (`identifier_number_{i}`)
  collapsed onto a handful of slots, and linear probing went quadratic: 10,608
  average probes per key for `identifier_number_{i}` at 100k keys, and 114 s to
  intern one million of them. The hash now ends in a xor-shift / multiply /
  xor-shift avalanche, so the slot index depends on every byte. The same
  1M-string workload takes 134 ms, and every tested corpus averages 1.3–1.6
  probes per key, matching the linear-probing expectation for the load factor.
- **`intern` documentation at the symbol-space bound (ledger M29).** The
  `Interner` docs still said a non-panicking exhaustion result was "scheduled for
  a later release", although `try_intern` has existed since 0.4.0. The rustdoc
  and [`docs/API.md`](docs/API.md#internerintern) now state exactly what `intern`
  does once all `u32::MAX` symbols are issued: it returns the highest symbol,
  which names the *last string interned*, not the new one. That behaviour is
  unchanged in 1.x because fixing it would lower the frozen capacity; it is
  recorded for 2.0 in [`dev/ROADMAP.md`](dev/ROADMAP.md). A test now pins it.

### Changed

- Short strings are hashed with fixed-width overlapping reads instead of
  copying a variable-length tail into a zeroed buffer, and the probe loop
  compares bytes rather than `&str` slices. Together these more than pay for
  the finalizer: on a pseudo-random short-identifier corpus where 1.0.0 did not
  cluster, hits are 11.6% faster and misses 50.9% faster.
- `tests/concurrent.rs` is compiled only with the `std` feature and the bench
  target declares `required-features = ["std"]`, so
  `cargo clippy --all-targets --no-default-features` builds again (both import
  the `std`-only `ConcurrentInterner`). No library code changed for this.
- Hash values and therefore the internal slot layout differ from 1.0.0. Symbol
  ids are unaffected: they are still assigned sequentially in interning order.
- The README performance table is re-measured: ~16 ns per repeat-hit `intern`
  and ~27 ns per new-string `intern`, replacing ~0.23 µs and ~0.62 µs, which
  measured the clustering defect rather than the interner. The 8-thread
  `ConcurrentInterner` figure is corrected from ~4× to ~2.3× single-thread
  throughput. The ratio fell because the single-thread path got much faster,
  so per-iteration thread-spawn cost now dominates the bench.

### Added (tests and benches only)

- A test-only probe counter, plus tests that bound average and worst-case probe
  counts (hits and misses) for 100k numbered, zero-padded, common-prefix, and
  common-suffix identifiers; the bench corpus; every one- and two-byte ASCII
  string; and 1 KiB strings. Also added: the regression test
  `h03_numbered_identifiers_do_not_cluster`, a property test over arbitrary
  `{prefix}{counter}{suffix}` families, a test that every byte position reaches
  the hash at lengths 0–24, and a full-width collision check on the structured
  corpora.
- Criterion `scale_intern_new` / `scale_intern_existing` groups at 10k, 100k,
  and 1M distinct strings over numbered, prefixed, bench-corpus, and
  pseudo-random corpora, and a `short_identifiers` hit/miss group.

---

## [1.0.0] - 2026-06-20

API freeze. The public surface is declared stable and frozen under Semantic
Versioning until 2.0. There are no code changes from `0.4.0`; this release marks
the contract.

### Changed

- `docs/API.md` is marked **stable**: the documented surface — `Symbol`,
  `Interner`, `ConcurrentInterner`, `Lookup`, `InternError` — is frozen, and no
  breaking change will be made without a major version bump. `InternError` remains
  `#[non_exhaustive]` and SemVer's additive rule still permits new items.
- The full property-test and benchmark suite is verified green on Linux, macOS,
  and Windows against stable and MSRV 1.85.

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

[Unreleased]: https://github.com/jamesgober/intern-lang/compare/v1.0.1...HEAD
[1.0.1]: https://github.com/jamesgober/intern-lang/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/jamesgober/intern-lang/compare/v0.4.0...v1.0.0
[0.4.0]: https://github.com/jamesgober/intern-lang/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/jamesgober/intern-lang/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/jamesgober/intern-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/intern-lang/releases/tag/v0.1.0
