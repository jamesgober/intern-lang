//! # intern_lang
//!
//! Fast string and symbol interning — identifier comparisons become integer
//! comparisons.
//!
//! `intern-lang` maps each distinct string to a small, copyable [`Symbol`],
//! stores the bytes once in a contiguous backing store, and hands back integer
//! handles. A compiler front-end interns every identifier once, then compares and
//! passes symbols instead of strings: a name comparison becomes an integer
//! comparison, and a name in an AST node costs four bytes instead of an owned
//! `String`. It is the deduplication layer beneath a lexer and a symbol table,
//! and it owns interning and nothing else — no lexing, no scoping, no I/O.
//!
//! ## At a glance
//!
//! - [`Interner`] — the single-threaded interner. [`intern`](Interner::intern)
//!   deduplicates and returns a stable [`Symbol`];
//!   [`resolve`](Interner::resolve) borrows the bytes back out of the store.
//! - [`Symbol`] — a four-byte `Copy` handle whose equality, ordering, and hashing
//!   are integer operations.
//!
//! ## Example
//!
//! ```
//! use intern_lang::Interner;
//!
//! let mut interner = Interner::new();
//!
//! // Interning the same string twice yields the same symbol...
//! let a = interner.intern("while");
//! let b = interner.intern("while");
//! assert_eq!(a, b);
//!
//! // ...and distinct strings yield distinct symbols.
//! let c = interner.intern("for");
//! assert_ne!(a, c);
//!
//! // Comparing names is now an integer compare; resolving borrows the bytes.
//! assert_eq!(interner.resolve(a), Some("while"));
//! assert_eq!(interner.resolve(c), Some("for"));
//! ```
//!
//! ## `no_std`
//!
//! The crate is `no_std`-compatible: it relies only on `alloc`, never on the
//! standard library. The default `std` feature is additive. Disable default
//! features to build for a `no_std` target.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(unused_must_use)]
#![deny(unused_results)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::unreachable)]

extern crate alloc;

mod interner;
mod symbol;

pub use interner::Interner;
pub use symbol::Symbol;
