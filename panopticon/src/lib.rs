//! The Parallax cockpit, as a library so its view model and rendering
//! are testable without a terminal.
//!
//! **Read-only.** Nothing here calls `parallax_baseline::actions`;
//! `tests/read_only.rs` asserts it over this crate's own source.
#![warn(missing_docs)]

pub mod keys;
pub mod view;
