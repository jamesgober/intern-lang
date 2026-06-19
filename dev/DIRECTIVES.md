# intern-lang &mdash; Engineering Directives

> Engineering standards and the definition of done for this project. Read alongside `REPS.md` (root, authoritative) and `dev/ROADMAP.md` (current phase). If anything here conflicts with `REPS.md`, `REPS.md` wins.

---

## 0. Philosophy

This library is built and maintained to a production standard and treated as a flagship piece of work. Plan the full path, then build one verified step at a time. "Good enough" is treated as a defect. intern-lang sits at the bottom of the language-tooling stack: a lexer interns every identifier and a symbol table keys on the result, so the cost of a single intern and the size of a `Symbol` are multiplied across an entire compilation.

---

## 1. What this is

intern-lang is a string interner. It maps each distinct string to a small, copyable `Symbol`, stores the bytes once in a contiguous backing store, and hands back integer handles so that comparing two names is an integer comparison rather than a byte walk. It is the deduplication layer beneath the lexer and the symbol table. It owns interning and nothing else: no lexing, no scoping, no I/O.

---

## 2. Engineering law (non-negotiable)

- **Performance** — peak is the baseline; a `Symbol` is a small `Copy` value; interning an already-seen string is a hash lookup with no allocation; resolved string slices borrow the store, never copy; no "faster" claim without `criterion` numbers.
- **Correctness** — the invariants in section 4 are covered by property tests, cross-checked against a `HashMap<String, u32>` reference interner.
- **Security** — the symbol space is bounded and exhaustion is a defined, non-panicking outcome; resolving a symbol never reads out of bounds; hostile input (huge strings, many strings) is handled without UB.
- **Architecture** — SOLID, KISS, YAGNI; one responsibility; the single-threaded and concurrent interners share one trait seam.
- **Cross-platform** — Linux/macOS/Windows first-class, verified by CI.
- **Error handling** — fallible paths (symbol-space exhaustion, resolving a foreign symbol) return `Result`/`Option` per the documented contract; nothing is silently wrong.
- **Production-ready** — `#![forbid(unsafe_code)]` (or every `unsafe` carries a `// SAFETY:` proof if the contiguous store requires it) and `#![deny(missing_docs)]` from the first commit; no stray `println!`/`dbg!`; every public item has rustdoc with a runnable example.

---

## 3. Definition of done

1. Compiles clean on Linux/macOS/Windows, stable and MSRV 1.85.
2. `fmt`, `clippy -D warnings`, `test --all-features`, `cargo doc -D warnings` clean.
3. `cargo audit` + `cargo deny check` pass.
4. No `unwrap`/`expect`/`todo!`/`dbg!` in shipping code; any `unsafe` justified with `// SAFETY:`.
5. A Tier-1 API exists and headlines the docs.
6. Property tests cover every section-4 invariant.
7. Hot-path changes carry benchmarks; no regression over 5%.
8. Docs and `CHANGELOG.md` updated; the matching `docs/release/vX.Y.Z.md` written before the tag.

---

## 4. Project-specific invariants

- Interning the same string twice returns the same `Symbol`; interning two distinct strings returns two distinct symbols (no collisions).
- `resolve(intern(s)) == s` for every string `s` the interner has accepted — round-trip fidelity, property-tested against a reference map.
- A `Symbol` stays valid and resolves to the same string for the entire lifetime of the interner that issued it; growth of the backing store never invalidates a previously issued symbol or a slice resolved from one within its documented borrow.
- `Symbol` is `Copy` and small; passing one is never more expensive than passing an integer.
- Symbol-space exhaustion is reported, never silently wrapped.
