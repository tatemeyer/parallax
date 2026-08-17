//! Parallax Baseline: the platform core. Holds every registered
//! project's declared references — manifests, the normalized autonomy
//! axes, adapters over work/verification/artifacts/sessions, aggregated
//! state with per-source freshness, and control actions.
//!
//! Deliberately **never touches a terminal**: no UI, no TTY, no
//! rendering. The cockpit is this library's first frontend, not its
//! only possible one.
#![warn(missing_docs)]

pub mod adapters;
pub mod autonomy;
pub mod freshness;
pub mod manifest;
pub mod state;
pub mod validate;
