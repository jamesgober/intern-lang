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
- [ ] Every public item has rustdoc + a runnable example.
- [ ] Dedup, distinctness, and `resolve(intern(s)) == s` round-trip property-tested against a `HashMap` reference interner.
- [ ] Symbol stability across many interns (forcing store growth) property-tested.

---

## v0.3.0 — Concurrent interner

A thread-safe interner that many lexer/parser threads can intern into at once,
behind the same trait seam as the single-threaded one so it is additive, not a
rewrite. Contention behaviour benchmarked, not assumed.

Exit criteria:
- [ ] Concurrent interning is correct under contention (no duplicate symbols for the same string across threads), proven not assumed.
- [ ] The single-threaded hot path is not taxed by the concurrent backend's existence.

---

## v0.4.0 — serde, exhaustion contract, feature freeze

Optional `serde` for `Symbol`, a defined symbol-space-exhaustion result, and a
declared frozen public surface.

Exit criteria:
- [ ] Exhaustion returns a defined error, property-tested at the boundary.
- [ ] `serde` round-trips `Symbol` under the feature.
- [ ] API surface documented as frozen in `docs/API.md`.

---

## v1.0.0 — API freeze

The interner/symbol surface is stable and frozen until 2.0. No new public API,
only documentation, tests, and internal optimisation.

Exit criteria:
- [ ] `docs/API.md` marked stable; SemVer promise recorded.
- [ ] Full property-test and benchmark suite green on all three platforms.
