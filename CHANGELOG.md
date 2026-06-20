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

[Unreleased]: https://github.com/jamesgober/intern-lang/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/jamesgober/intern-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/intern-lang/releases/tag/v0.1.0
