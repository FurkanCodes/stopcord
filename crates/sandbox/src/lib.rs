//! Confines one terminal.
//!
//! A [`Policy`] says what a terminal's agent may touch. On macOS it becomes a Seatbelt profile run
//! through `sandbox-exec`, on Linux a bubblewrap command line. Web traffic goes through a local [`Filter`]
//! that only reaches allowed domains. `docs/sandbox-spike.md` has the reasoning behind both.

mod bridge;
mod bwrap;
mod filter;
mod paths;
mod policy;
mod seatbelt;
mod wrap;

pub use bridge::bridge_main;
pub use bwrap::bwrap_args;
pub use filter::{Denied, Filter};
pub use policy::{COMMON_DENY_READS, Network, Policy, Reads};
pub use seatbelt::seatbelt_profile;
pub use wrap::{Wrapped, available, wrap};
