//! Integration tests exercising the public interner surface and its edge cases.

use intern_lang::Interner;

#[test]
fn test_intern_deduplicates_repeated_strings() {
    let mut interner = Interner::new();
    let first = interner.intern("repeat");
    for _ in 0..1_000 {
        assert_eq!(interner.intern("repeat"), first);
    }
    assert_eq!(interner.len(), 1);
}

#[test]
fn test_distinct_strings_get_distinct_symbols() {
    let mut interner = Interner::new();
    let words = ["fn", "let", "mut", "impl", "trait", "where"];
    let symbols: Vec<_> = words.iter().map(|w| interner.intern(w)).collect();

    for (i, &a) in symbols.iter().enumerate() {
        for &b in &symbols[i + 1..] {
            assert_ne!(a, b);
        }
    }
    assert_eq!(interner.len(), words.len());
}

#[test]
fn test_resolve_returns_original_bytes() {
    let mut interner = Interner::new();
    let cases = ["", "x", "snake_case", "with spaces", "café", "🦀"];
    let symbols: Vec<_> = cases.iter().map(|s| interner.intern(s)).collect();
    for (sym, expected) in symbols.iter().zip(cases) {
        assert_eq!(interner.resolve(*sym), Some(expected));
    }
}

#[test]
fn test_empty_interner_resolves_nothing() {
    let mut issuer = Interner::new();
    let sym = issuer.intern("only");

    let empty = Interner::new();
    assert_eq!(empty.resolve(sym), None);
}

#[test]
fn test_get_is_read_only() {
    let mut interner = Interner::new();
    assert_eq!(interner.get("missing"), None);
    assert!(interner.is_empty());

    let sym = interner.intern("present");
    assert_eq!(interner.get("present"), Some(sym));
    assert_eq!(interner.len(), 1);
}

#[test]
fn test_large_volume_roundtrips() {
    let mut interner = Interner::new();
    let mut symbols = Vec::new();
    for i in 0..50_000 {
        let s = format!("ident_{i}");
        symbols.push((interner.intern(&s), s));
    }
    assert_eq!(interner.len(), 50_000);
    for (sym, s) in &symbols {
        assert_eq!(interner.resolve(*sym), Some(s.as_str()));
    }
}

#[test]
fn test_long_strings() {
    let mut interner = Interner::new();
    let long = "a".repeat(100_000);
    let sym = interner.intern(&long);
    assert_eq!(interner.resolve(sym), Some(long.as_str()));
    assert_eq!(interner.intern(&long), sym);
}

#[test]
fn test_with_capacity_does_not_change_results() {
    let words: Vec<String> = (0..2_000).map(|i| format!("w{i}")).collect();

    let mut sized = Interner::with_capacity(2_000);
    let mut grown = Interner::new();

    for w in &words {
        assert_eq!(sized.intern(w).as_u32(), grown.intern(w).as_u32());
    }
    assert_eq!(sized.len(), grown.len());
}

#[test]
fn test_symbols_usable_as_map_keys() {
    use std::collections::HashMap;

    let mut interner = Interner::new();
    let mut types: HashMap<_, &str> = HashMap::new();
    let _ = types.insert(interner.intern("i32"), "integer");
    let _ = types.insert(interner.intern("f64"), "float");

    assert_eq!(types.get(&interner.intern("i32")), Some(&"integer"));
    assert_eq!(types.get(&interner.intern("f64")), Some(&"float"));
}
