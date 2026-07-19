//! A structural reader + differ for `.class` files: the physical-divergence
//! localization tool used by the exact-byte fixture harness.
//!
//! njavac retains javac's bytes whenever practical. When they diverge, the benchmark's
//! `javap -v` text diff is a good first look but goes blind
//! exactly when it matters — it can't see a byte the disassembler normalizes away
//! ("bytes differ but javap output matches"), and it reports the *first differing
//! line of text*, which for a one-entry constant-pool shift is a cascade of
//! symptoms far from the cause.
//!
//! This crate is the independent reader counterpart to `njavac-compiler`'s
//! class-file writer. It walks the class-file format and produces a flat, ordered
//! list of **fields**, each tagged with its byte offset and a structural path
//! (`methods[0].attr[0].Code.max_stack`).
//! Diffing two such lists in lock-step finds the *first structural divergence with
//! a byte offset*, which localizes to the cause and works even when javap agrees.
//!
//! It recognizes the standard constant-pool tags needed by current javac output
//! and structurally decodes `Code`, `LineNumberTable`, `StackMapTable`, and
//! `SourceFile`. Other attribute bodies and the raw code array are captured as hex
//! fields and bounded by their declared lengths, so an unfamiliar attribute does
//! not derail the surrounding parse. Utf8 display is currently lossy standard
//! UTF-8 rather than a complete modified-UTF-8 decoder.

mod diff;
mod reader;

pub use diff::{diff_report, render_dump};
pub use reader::{dump, Field};
