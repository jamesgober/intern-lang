//! Serde round-trip tests for `Symbol`, gated on the `serde` feature.

#![cfg(feature = "serde")]

use intern_lang::{Interner, Symbol};
use proptest::prelude::*;

#[test]
fn test_interned_symbol_json_roundtrips() {
    let mut interner = Interner::new();
    let sym = interner.intern("roundtrip");

    let json = serde_json::to_string(&sym).unwrap();
    let back: Symbol = serde_json::from_str(&json).unwrap();

    assert_eq!(back, sym);
    assert_eq!(interner.resolve(back), Some("roundtrip"));
}

#[test]
fn test_symbol_deserializes_from_a_bare_integer() {
    // The wire form is just the id, so a hand-written integer deserializes.
    let sym: Symbol = serde_json::from_str("7").unwrap();
    assert_eq!(sym.as_u32(), 7);
}

#[test]
fn test_zero_is_rejected_on_deserialize() {
    // `Symbol` is backed by a `NonZeroU32`, so `0` is not a valid id.
    assert!(serde_json::from_str::<Symbol>("0").is_err());
}

proptest! {
    /// Every valid symbol id survives a JSON round-trip unchanged.
    #[test]
    fn symbol_json_roundtrips_for_any_id(id in 1u32..=u32::MAX) {
        let sym = Symbol::from_u32(id).unwrap();
        let json = serde_json::to_string(&sym).unwrap();
        let back: Symbol = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(back, sym);
        prop_assert_eq!(back.as_u32(), id);
    }

    /// A symbol serializes as exactly its integer id — a stable wire contract.
    #[test]
    fn symbol_serializes_as_its_id(id in 1u32..=u32::MAX) {
        let sym = Symbol::from_u32(id).unwrap();
        prop_assert_eq!(serde_json::to_string(&sym).unwrap(), id.to_string());
    }
}
