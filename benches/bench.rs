//! Criterion benchmarks for the interner hot paths.
//!
//! The two paths that matter most are interning a string that has already been
//! seen (a hash lookup with no allocation — the common case once a file is warm)
//! and interning a fresh string (which appends to the store and may grow the
//! index). Resolution is the read path a symbol table leans on.
//!
//! The `scale_*` groups run those paths at 10k / 100k / 1M distinct strings over
//! several corpus shapes. Small, uniform-looking corpora hid H03 (structured keys
//! such as numbered identifiers clustering onto a few home slots), so the scale
//! groups deliberately include numbered and common-prefix corpora alongside a
//! pseudo-random control corpus that any reasonable hash spreads well.

use std::hint::black_box;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use std::collections::HashSet;

use criterion::{BatchSize, Criterion, SamplingMode, Throughput, criterion_group, criterion_main};
use intern_lang::{ConcurrentInterner, Interner};

/// A spread of identifier-shaped strings of varying length.
fn corpus(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("identifier_number_{i}")).collect()
}

/// Distinct pseudo-random identifiers of 1..=12 bytes (`[a-z_][a-z0-9_]*`),
/// generated deterministically. This is the control corpus: its bytes vary from
/// the first position, so even a weak hash spreads it, and it measures plain
/// short-identifier throughput rather than clustering behaviour.
fn mixed_corpus(n: usize) -> Vec<String> {
    const HEAD: &[u8] = b"abcdefghijklmnopqrstuvwxyz_";
    const TAIL: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789_";
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        // xorshift64*: deterministic, dependency-free, good enough for test data.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        state.wrapping_mul(0x2545_f491_4f6c_dd1d)
    };
    let mut seen = HashSet::with_capacity(n);
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        let r = next();
        let len = 1 + (r % 12) as usize;
        let mut s = String::with_capacity(len);
        s.push(char::from(HEAD[(next() % HEAD.len() as u64) as usize]));
        for _ in 1..len {
            s.push(char::from(TAIL[(next() % TAIL.len() as u64) as usize]));
        }
        if seen.insert(s.clone()) {
            out.push(s);
        }
    }
    out
}

/// Builds a corpus of `n` distinct strings.
type CorpusFn = fn(usize) -> Vec<String>;

/// The corpus shapes the scale groups run over: `(name, generator)`.
fn scale_corpora() -> [(&'static str, CorpusFn); 4] {
    [
        // Short numbered identifiers: the exact H03 shape.
        ("numbered", |n| (0..n).map(|i| format!("t{i}")).collect()),
        // A long shared prefix, as with qualified paths or mangled names.
        ("prefixed", |n| {
            (0..n)
                .map(|i| format!("very::long::common::module::path::item_{i}"))
                .collect()
        }),
        // This file's own historical corpus, also clustered by the 1.0.0 hash.
        ("identifier_number", corpus),
        // Control: pseudo-random short identifiers.
        ("mixed", mixed_corpus),
    ]
}

const SCALES: [usize; 3] = [10_000, 100_000, 1_000_000];

/// Interning `n` new strings into a fresh interner (growth included).
fn bench_scale_intern_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("scale_intern_new");
    group.sample_size(10);
    group.sampling_mode(SamplingMode::Flat);
    for (name, make) in scale_corpora() {
        for n in SCALES {
            group.throughput(Throughput::Elements(n as u64));
            // Setup lives inside the closure so a filtered-out bench costs nothing.
            group.bench_function(format!("{name}/{n}"), |b| {
                let words = make(n);
                b.iter_batched(
                    Interner::new,
                    |mut interner| {
                        for w in &words {
                            let _ = black_box(interner.intern(w));
                        }
                        interner
                    },
                    BatchSize::LargeInput,
                );
            });
        }
    }
    group.finish();
}

/// Re-interning `n` strings that are all already present (the warm hit path).
fn bench_scale_intern_existing(c: &mut Criterion) {
    let mut group = c.benchmark_group("scale_intern_existing");
    group.sample_size(10);
    group.sampling_mode(SamplingMode::Flat);
    for (name, make) in scale_corpora() {
        for n in SCALES {
            group.throughput(Throughput::Elements(n as u64));
            group.bench_function(format!("{name}/{n}"), |b| {
                let words = make(n);
                let mut interner = Interner::with_capacity(n);
                for w in &words {
                    let _ = interner.intern(w);
                }
                b.iter(|| {
                    for w in &words {
                        let _ = black_box(interner.intern(w));
                    }
                });
            });
        }
    }
    group.finish();
}

/// Short-identifier hit and miss throughput on the control corpus at a
/// cache-resident size: the guard that the H03 fix did not tax the common case.
fn bench_short_identifiers(c: &mut Criterion) {
    let words = mixed_corpus(2_048);
    let (present, absent) = words.split_at(1_024);
    let mut interner = Interner::new();
    for w in present {
        let _ = interner.intern(w);
    }

    let mut group = c.benchmark_group("short_identifiers");
    group.throughput(Throughput::Elements(1_024));
    group.bench_function("hit/1024", |b| {
        b.iter(|| {
            for w in present {
                let _ = black_box(interner.intern(w));
            }
        });
    });
    group.bench_function("miss/1024", |b| {
        b.iter(|| {
            for w in absent {
                let _ = black_box(interner.get(w));
            }
        });
    });
    group.finish();
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

/// Throughput of the warm read path (interning strings already present) as the
/// thread count rises. Since hits are served under a shared read lock, this shows
/// how the concurrent interner scales when most identifiers are already known.
fn bench_concurrent_intern_existing(c: &mut Criterion) {
    let words = Arc::new(corpus(1_024));
    let interner = Arc::new(ConcurrentInterner::new());
    for w in words.iter() {
        let _ = interner.intern(w);
    }

    let mut group = c.benchmark_group("concurrent_intern_existing");
    for &threads in &[1usize, 4, 8] {
        group.bench_function(format!("{threads}t/1024"), |b| {
            b.iter_custom(|iters| {
                let start = Instant::now();
                for _ in 0..iters {
                    let handles: Vec<_> = (0..threads)
                        .map(|_| {
                            let interner = Arc::clone(&interner);
                            let words = Arc::clone(&words);
                            thread::spawn(move || {
                                for w in words.iter() {
                                    let _ = black_box(interner.intern(w));
                                }
                            })
                        })
                        .collect();
                    for handle in handles {
                        let _ = handle.join();
                    }
                }
                start.elapsed()
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_intern_existing,
    bench_intern_new,
    bench_resolve,
    bench_get_miss,
    bench_concurrent_intern_existing,
    bench_short_identifiers,
    bench_scale_intern_new,
    bench_scale_intern_existing
);
criterion_main!(benches);
