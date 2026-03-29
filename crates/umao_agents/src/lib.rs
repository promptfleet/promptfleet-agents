//! # umao_agents
//!
//! UMAO coordination for promptfleet A2A agents.
//!
//! This crate bridges the vendor-neutral UMAO orchestration engine with the
//! promptfleet agent SDK, providing [`PromptFleetExecutor`] — an
//! [`AsyncNodeExecutor`] implementation that dispatches Delegate nodes to
//! real A2A agents via [`agent_sdk::A2aClient`].
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use umao_agents::PromptFleetExecutor;
//! use umao_executor::orchestrator;
//!
//! // Register agents by name → endpoint
//! let mut executor = PromptFleetExecutor::new();
//! executor.register("researcher", "http://localhost:3001");
//! executor.register("synthesizer", "http://localhost:3002");
//!
//! // Load graph, run orchestration
//! // let result = orchestrator::execute_async(&graph, &executor, None).await;
//! ```
//!
//! For LLM-powered graph planning and smart agent orchestration,
//! try [promptfleet cloud](https://promptfleet.io) (free tier).

mod executor;

pub use executor::PromptFleetExecutor;

pub use umao_core;
pub use umao_executor;
