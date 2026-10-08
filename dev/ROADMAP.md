# intern-lang — Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

---

## v0.1.0 — Scaffold (DONE)

Compiles, CI green, structure correct, no domain logic.

- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt.
- [x] API surface sketched in `docs/API.md`.

---

## v0.2.0 — Core interner & symbol (THE HARD PART, NOT DEFERRED)

`Interner` with `intern(&str) -> Symbol` and `resolve(Symbol) -> &str`, plus a
`Copy` `Symbol`. The genuinely hard part is the storage design, front-loaded
now: bytes live once in a contiguous backing store, deduplicated through a hash
index, and a `Symbol` issued early must keep resolving correctly after the store
grows — symbol stability across growth is the invariant everything above depends
on, so it is proven here rather than after the easy `intern`/`resolve` surface.

Exit criteria:
- [x] Every public item has rustdoc + a runnable example.
- [x] Dedup, distinctness, and `resolve(intern(s)) == s` round-trip property-tested against a `HashMap` reference interner.
- [x] Symbol stability across many interns (forcing store growth) property-tested.

---

## v0.3.0 — Concurrent interner

A thread-safe interner that many lexer/parser threads can intern into at once,
behind the same trait seam as the single-threaded one so it is additive, not a
rewrite. Contention behaviour benchmarked, not assumed.

Exit criteria:
- [x] Concurrent interning is correct under contention (no duplicate symbols for the same string across threads), proven not assumed.
- [x] The single-threaded hot path is not taxed by the concurrent backend's existence.

---

## v0.4.0 — serde, exhaustion contract, feature freeze

Optional `serde` for `Symbol`, a defined symbol-space-exhaustion result, and a
declared frozen public surface.

Exit criteria:
- [x] Exhaustion returns a defined error, property-tested at the boundary.
- [x] `serde` round-trips `Symbol` under the feature.
- [x] API surface documented as frozen in `docs/API.md`.

---

## v1.0.0 — API freeze

The interner/symbol surface is stable and frozen until 2.0. No new public API,
only documentation, tests, and internal optimisation.

Exit criteria:
- [x] `docs/API.md` marked stable; SemVer promise recorded.
- [x] Full property-test and benchmark suite green on all three platforms.

---

## v1.0.1 — Patch: slot-selection quality (H03) and exhaustion docs (M29) (DONE)

A patch on the frozen 1.0 surface: no public item added, removed, or changed.
Fixes ledger items H03 and M29 from `_lexersketch/ISSUES.md`.

Delivered:
- [x] **H03 — clustered slot selection.** The 1.0.0 dedup index took the home
  slot from the *low* bits of an FxHash-style multiply with no finalizer. A
  multiply only carries information upward, so the low `k` bits depended only on
  the low `k` bits of the last word: numbered identifiers (`t0000..t7999`,
  `var0000..`), common-prefix names, and the crate's own benchmark corpus
  (`identifier_number_{i}`) collapsed onto a handful of home slots and every
  insert became a long linear walk (measured: 10,608 average probes per key for
  `identifier_number_{i}` at 100k; 1,152 for `t0000..t7999`; 114 s to intern 1M
  of the bench corpus). `hash_bytes` now ends in a xor-shift / multiply /
  xor-shift avalanche (64-bit ops only), so every slot-index bit depends on every
  byte; the cached fingerprint moved to the high 32 bits. A folded 128-bit
  multiply was tried first and rejected: it left single-byte keys at 3.4 average
  probes (lattice structure from multiply-of-a-multiply). Short strings are now
  packed with fixed-width overlapping reads instead of copying a variable-length
  tail, and the probe compares bytes instead of `&str`; together these more than
  pay for the finalizer (short-identifier hits −11.6%, misses −50.9% on a
  corpus 1.0.0 did not cluster). Average probes per key are now 1.3–1.6 on every
  tested corpus; 1M bench-corpus interning takes 134 ms.
- [x] Test-only probe counter (`Interner::probe_count`, `#[cfg(test)]`, re-walks
  the probe sequence so the hot path carries no counter) and unit tests that bound
  average and worst-case probes (hit and miss) for 100k numbered ids, padded
  numbered ids, common-prefix ids, common-suffix ids, the bench corpus,
  every one- and two-byte ASCII string, and 1 KiB strings; the named regression
  test `h03_numbered_identifiers_do_not_cluster`; a property test over arbitrary
  `{prefix}{counter}{suffix}` families; a test that every byte reaches the hash at
  lengths 0–24; and a full-width collision check. Six of these fail with the
  finalizer removed.
- [x] Criterion `scale_intern_new` / `scale_intern_existing` groups at 10k / 100k
  / 1M distinct strings over numbered, prefixed, bench-corpus, and pseudo-random
  control corpora, and a `short_identifiers` hit/miss group guarding the common
  case (issue F16: toy-scale benches hid H03).
- [x] **M29 — stale and imprecise exhaustion docs.** Removed the stale "defined,
  non-panicking exhaustion result is scheduled for a later release" text
  (`try_intern` has existed since 0.4.0) and documented exactly what `intern`
  does at the symbol-space bound, in the rustdoc and `docs/API.md`, with a test
  pinning that behaviour.
- [x] Gate fix: `cargo clippy --all-targets --no-default-features` failed at
  1.0.0 because `tests/concurrent.rs` and the bench import the `std`-only
  `ConcurrentInterner`; the test file is now `#![cfg(feature = "std")]` and the
  bench declares `required-features = ["std"]`.

Dependency wiring: unchanged — zero runtime dependencies (optional `serde` only),
no first-party crates wired.

### Moved out of this patch (anti-deferral record)

- **`intern()` at the bound returns a symbol that names another string (M29).**
  At `u32::MAX` issued symbols every id already names a string, so no return value
  of type `Symbol` can mean "not interned". Reserving `u32::MAX` as a sentinel
  would lower the documented, frozen capacity (`u32::MAX` distinct strings) and
  move `try_intern`'s error boundary, which is a change to frozen behaviour, not a
  bug fix a patch may make. 1.x therefore keeps the saturating behaviour, now
  documented exactly. **Planned for 2.0:** either make `intern` return
  `Result`/`Option`, or reserve a sentinel id and lower the documented capacity by
  one, decided together with the rest of the 2.0 wave.
- **`Clone`, `iter`, and `serde` for `Interner` (rest of M29).** These are new
  public API. SemVer allows them only in a minor release, so they are recorded for
  **1.1.0**, not this patch.
- **Hash flooding (not part of H03).** The hash is unkeyed and deterministic. The
  finalizer fixes *accidental* clustering of structured keys; it does not stop an
  adversary who deliberately builds colliding strings for this exact function, which
  still degrades the index to linear probing over the colliding set. A keyed hash
  needs a per-interner seed, and `no_std` plus zero-dependency leaves no portable
  entropy source; the options (caller-supplied seed constructor in 1.x as additive
  API, or a seeded default in 2.0) are recorded for the 1.1.0 / 2.0 design.
