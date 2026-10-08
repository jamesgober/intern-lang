//! Integration tests for the thread-safe `ConcurrentInterner`, focused on
//! correctness under contention.
//!
//! `ConcurrentInterner` exists only with the `std` feature, so the whole file is
//! compiled out under `--no-default-features`.
#![cfg(feature = "std")]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;

use intern_lang::{ConcurrentInterner, Lookup};

/// Many threads interning a heavily overlapping set of strings must all agree on
/// the symbol for each string: no string is ever assigned two different symbols.
#[test]
fn test_overlapping_interning_yields_one_symbol_per_string() {
    const THREADS: usize = 16;
    const DISTINCT: usize = 500;

    let interner = Arc::new(ConcurrentInterner::new());
    // 2000 interns per thread over 500 distinct strings — lots of contention.
    let words: Arc<Vec<String>> = Arc::new(
        (0..2_000)
            .map(|i| format!("name_{}", i % DISTINCT))
            .collect(),
    );

    let handles: Vec<_> = (0..THREADS)
        .map(|_| {
            let interner = Arc::clone(&interner);
            let words = Arc::clone(&words);
            thread::spawn(move || {
                let mut local: HashMap<String, _> = HashMap::new();
                for w in words.iter() {
                    let _ = local.insert(w.clone(), interner.intern(w));
                }
                local
            })
        })
        .collect();

    let maps: Vec<HashMap<String, _>> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // Every thread observed the same symbol for every string.
    let reference = &maps[0];
    for map in &maps[1..] {
        for (word, symbol) in map {
            assert_eq!(reference.get(word), Some(symbol));
        }
    }

    // Distinctness across the whole run: 500 strings -> 500 distinct symbols.
    let symbols: HashSet<_> = reference.values().copied().collect();
    assert_eq!(symbols.len(), DISTINCT);
    assert_eq!(interner.len(), DISTINCT);
}

/// Every distinct string interned across threads round-trips through `resolve`.
#[test]
fn test_concurrent_interning_resolves_correctly() {
    const THREADS: usize = 8;
    const PER_THREAD: usize = 1_000;

    let interner = Arc::new(ConcurrentInterner::new());

    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let interner = Arc::clone(&interner);
            thread::spawn(move || {
                let mut pairs = Vec::with_capacity(PER_THREAD);
                for i in 0..PER_THREAD {
                    // Disjoint per-thread namespaces guarantee distinct strings.
                    let s = format!("t{t}_s{i}");
                    pairs.push((interner.intern(&s), s));
                }
                pairs
            })
        })
        .collect();

    let all: Vec<(_, String)> = handles
        .into_iter()
        .flat_map(|h| h.join().unwrap())
        .collect();

    assert_eq!(interner.len(), THREADS * PER_THREAD);
    for (symbol, expected) in &all {
        assert_eq!(
            interner.resolve(*symbol).as_deref(),
            Some(expected.as_str())
        );
    }
}

/// `get` from one thread sees what another thread interned (once joined).
#[test]
fn test_get_observes_other_threads_interns() {
    let interner = Arc::new(ConcurrentInterner::new());

    let writer = {
        let interner = Arc::clone(&interner);
        thread::spawn(move || interner.intern("written_elsewhere"))
    };
    let symbol = writer.join().unwrap();

    assert_eq!(interner.get("written_elsewhere"), Some(symbol));
    assert_eq!(
        interner.resolve(symbol).as_deref(),
        Some("written_elsewhere")
    );
}

/// The concurrent interner satisfies the shared `Lookup` seam, so generic code
/// written against `&impl Lookup` works against it.
#[test]
fn test_usable_through_lookup_trait() {
    fn first_byte(interner: &impl Lookup, s: &str) -> Option<u8> {
        let sym = interner.get(s)?;
        interner.resolve_with(sym, |resolved| resolved.as_bytes()[0])
    }

    let interner = ConcurrentInterner::new();
    let _ = interner.intern("zebra");
    assert_eq!(first_byte(&interner, "zebra"), Some(b'z'));
    assert_eq!(first_byte(&interner, "missing"), None);
}

/// A `ConcurrentInterner` can be shared across threads by value behind `Arc`,
/// which requires `Send + Sync`.
#[test]
fn test_send_sync_bounds() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ConcurrentInterner>();
}
