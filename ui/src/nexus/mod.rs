//! DreamtecLabs Nexus frontend extension layer.
//!
//! Keep Nexus-owned presentation code isolated here so the PDM engine and
//! upstream UI remain easy to synchronize.

mod home;
mod infrastructure;
mod inventory;
mod storage;

pub use home::NexusHome;
pub use infrastructure::{NexusDeployments, NexusInfrastructure};
pub use inventory::NexusInventory;
pub use storage::NexusStorage;
