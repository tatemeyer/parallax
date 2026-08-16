//! Plumb's deterministic half: config parsing, scenario selection,
//! capture adapters, blinded prompt construction, finding merge,
//! ruling suppression, and verdict rendering. Deliberately owns
//! everything that can be unit-tested; subagent dispatch belongs to
//! the orchestrating skill, not here.
#![warn(missing_docs)]

pub mod adapter;
pub mod color;
pub mod config;
pub mod contact;
pub mod encode;
pub mod evidence;
pub mod finding;
pub mod glyph;
pub mod keys;
pub mod manifest;
pub mod merge;
pub mod prompt;
pub mod render;
pub mod report;
pub mod rulings;
pub mod script;
pub mod select;
pub mod verdict;
