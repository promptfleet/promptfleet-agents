//! Domain Services for A2A Protocol Core
//!
//! This module contains domain services that implement business logic
//! for A2A protocol operations while maintaining pure domain architecture.

pub mod task_storage;

pub use task_storage::{ConversationContext, InMemoryTaskStorage, TaskStorage};
