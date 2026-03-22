//! A2A v1.0 Standard Methods

#[cfg(feature = "protocol-core")]
pub mod messaging;

#[cfg(feature = "protocol-core")]
pub mod tasks;

#[cfg(feature = "protocol-core")]
pub mod params;

pub mod discovery;

#[cfg(feature = "protocol-core")]
pub use messaging::*;

#[cfg(feature = "protocol-core")]
pub use tasks::*;

#[cfg(feature = "protocol-core")]
pub use params::*;

pub use discovery::*;
