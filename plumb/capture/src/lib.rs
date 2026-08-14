//! Plumb's deterministic half: config parsing, scenario selection,
//! capture adapters, blinded prompt construction, finding merge,
//! ruling suppression, and verdict rendering. Deliberately owns
//! everything that can be unit-tested; subagent dispatch belongs to
//! the orchestrating skill, not here.
#![warn(missing_docs)]

pub mod adapter;
pub mod config;
pub mod manifest;
pub mod select;
