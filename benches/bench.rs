//! Criterion benchmarks for the interner hot paths.
//!
//! The two paths that matter most are interning a string that has already been
//! seen (a hash lookup with no allocation — the common case once a file is warm)
//! and interning a fresh string (which appends to the store and may grow the
//! index). Resolution is the read path a symbol table leans on.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use intern_lang::Interner;

/// A spread of identifier-shaped strings of varying length.
fn corpus(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("identifier_number_{i}")).collect()
}

fn bench_intern_existing(c: &mut Criterion) {
    let words = corpus(1_024);
    let mut interner = Interner::new();
    for w in &words {
        let _ = interner.intern(w);
    }

    c.bench_function("intern_existing/1024", |b| {
        b.iter(|| {
            for w in &words {
                let _ = black_box(interner.intern(w));
            }
        });
    });
}

fn bench_intern_new(c: &mut Criterion) {
    let words = corpus(1_024);

    c.bench_function("intern_new/1024", |b| {
        b.iter_batched(
            Interner::new,
            |mut interner| {
                for w in &words {
                    let _ = black_box(interner.intern(w));
                }
                interner
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_resolve(c: &mut Criterion) {
    let words = corpus(1_024);
    let mut interner = Interner::new();
    let symbols: Vec<_> = words.iter().map(|w| interner.intern(w)).collect();

    c.bench_function("resolve/1024", |b| {
        b.iter(|| {
            for &sym in &symbols {
                let _ = black_box(interner.resolve(sym));
            }
        });
    });
}

fn bench_get_miss(c: &mut Criterion) {
    let mut interner = Interner::new();
    for w in &corpus(1_024) {
        let _ = interner.intern(w);
    }
    let absent = corpus(1_024)
        .iter()
        .map(|w| format!("{w}_absent"))
        .collect::<Vec<_>>();

    c.bench_function("get_miss/1024", |b| {
        b.iter(|| {
            for w in &absent {
                let _ = black_box(interner.get(w));
            }
        });
    });
}

criterion_group!(
    benches,
    bench_intern_existing,
    bench_intern_new,
    bench_resolve,
    bench_get_miss
);
criterion_main!(benches);
