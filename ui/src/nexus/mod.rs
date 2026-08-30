//! DreamtecLabs Nexus frontend extension layer.
//!
//! Keep Nexus-owned presentation code isolated here so the PDM engine and
//! upstream UI remain easy to synchronize.

mod domains;
mod home;
mod inventory;
mod storage;

pub use domains::NexusDomains;
pub use home::NexusHome;
pub use inventory::NexusInventory;
pub use storage::NexusStorage;
