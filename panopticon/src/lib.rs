//! The Parallax cockpit, as a library so its view model and rendering
//! are testable without a terminal.
//!
//! **Read-only.** Nothing here calls `parallax_baseline::actions`;
//! `tests/read_only.rs` asserts it over this crate's own source.
#![warn(missing_docs)]

pub mod app;
pub mod bell;
pub mod fixtures;
pub mod keys;
pub mod refresh;
pub mod view;
