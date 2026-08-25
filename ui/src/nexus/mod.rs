//! DreamtecLabs Nexus frontend extension layer.
//!
//! Keep Nexus-owned presentation code isolated here so the PDM engine and
//! upstream UI remain easy to synchronize.

mod home;
mod inventory;

pub use home::NexusHome;
pub use inventory::NexusInventory;
