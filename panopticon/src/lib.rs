//! The Parallax cockpit, as a library so its view model and rendering
//! are testable without a terminal.
//!
//! **Observation is read-only.** Nothing outside `control` calls
//! `parallax_baseline::actions`; `tests/read_only.rs` asserts it over
//! this crate's own source. Every render path is therefore structurally
//! incapable of mutating anything, which is what makes the rest of the
//! screen safe to leave running.
#![warn(missing_docs)]

pub mod app;
pub mod bell;
pub mod control;
pub mod courier;
pub mod fixtures;
pub mod keys;
pub mod refresh;
pub mod view;
