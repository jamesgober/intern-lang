//! Property tests for the section-4 invariants, cross-checked against a
//! `HashMap<String, Symbol>` reference interner.

use std::collections::{HashMap, HashSet};

use intern_lang::{Interner, Symbol};
use proptest::prelude::*;

/// A trivial reference interner: a `HashMap` keyed by the owned string. The real
/// interner must agree with it on every observable outcome.
#[derive(Default)]
struct Reference {
    map: HashMap<String, Symbol>,
}

impl Reference {
    fn record(&mut self, s: &str, sym: Symbol) -> Option<Symbol> {
        self.map.insert(s.to_owned(), sym)
    }
}

proptest! {
    /// Dedup, distinctness, and round-trip fidelity, all checked in one pass
    /// against the reference map: the same string always returns the same symbol,
    /// distinct strings never share a symbol, and every interned string resolves
    /// back to itself.
    #[test]
    fn dedup_distinctness_and_roundtrip(inputs in proptest::collection::vec(".*", 0..256)) {
        let mut interner = Interner::new();
        let mut reference = Reference::default();
        let mut symbol_to_string: HashMap<Symbol, String> = HashMap::new();

        for s in &inputs {
            let sym = interner.intern(s);

            // Round-trip: the symbol resolves to exactly the bytes interned.
            prop_assert_eq!(interner.resolve(sym), Some(s.as_str()));

            match reference.record(s, sym) {
                // Seen before: must dedup to the same symbol as last time.
                Some(previous) => prop_assert_eq!(sym, previous),
                // First time: the symbol must be new (distinctness).
                None => {
                    if let Some(existing) = symbol_to_string.insert(sym, s.clone()) {
                        prop_assert_eq!(&existing, s);
                    }
                }
            }
        }

        // `len` counts exactly the distinct strings.
        prop_assert_eq!(interner.len(), reference.map.len());
    }

    /// `get` agrees with `intern` without ever creating a symbol.
    #[test]
    fn get_matches_intern_without_growth(inputs in proptest::collection::vec("[a-z]{0,8}", 0..128)) {
        let mut interner = Interner::new();
        let mut interned: HashSet<String> = HashSet::new();

        for s in &inputs {
            match interner.get(s) {
                Some(sym) => {
                    prop_assert!(interned.contains(s));
                    prop_assert_eq!(interner.intern(s), sym);
                }
                None => {
                    prop_assert!(!interned.contains(s));
                    let sym = interner.intern(s);
                    let _ = interned.insert(s.clone());
                    prop_assert_eq!(interner.get(s), Some(sym));
                }
            }
        }
    }

    /// Symbol stability across store growth: symbols issued early keep resolving
    /// to their original strings after many more interns force the backing buffer
    /// and the dedup index to reallocate.
    #[test]
    fn symbols_stay_valid_after_growth(
        early in proptest::collection::vec("[a-z]{1,24}", 1..64),
        later in proptest::collection::vec("[a-z]{1,24}", 0..2048),
    ) {
        let mut interner = Interner::new();

        let mut remembered: Vec<(Symbol, String)> = Vec::new();
        for s in &early {
            remembered.push((interner.intern(s), s.clone()));
        }

        // Force growth with a flood of further interns.
        for s in &later {
            let _ = interner.intern(s);
        }

        // Every early symbol still resolves to the exact string it named.
        for (sym, s) in &remembered {
            prop_assert_eq!(interner.resolve(*sym), Some(s.as_str()));
        }
    }
}
