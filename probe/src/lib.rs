//! `parallax-probe` — serves one machine's platform state.
//!
//! Run it on every machine that holds projects. It scans that machine's
//! own disk with the adapters `parallax-baseline` already provides and
//! serves the aggregated result as JSON on loopback. A cockpit anywhere
//! on the tailnet fetches it and merges it with its own.
//!
//! ```text
//! parallax-probe --projects-root ~/Dev
//! tailscale serve --bg --https=443 http://127.0.0.1:8737
//! ```
//!
//! It never binds a routable address; see [`server::bind_address`].
//!
//! A library as well as a binary so an integration test can bind a real
//! ephemeral port and drive the whole round trip — probe serves, client
//! fetches, observation is re-stamped — through the same code the
//! command line runs. A test that reached only into a binary's private
//! modules would be testing something adjacent to what ships.

#![warn(missing_docs)]

pub mod server;
pub mod state;
