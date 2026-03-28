//! UMAO local orchestrator — loads a graph JSON and executes it against local agents.
//!
//! ## Three-tab demo
//!
//! ```bash
//! # Tab 1: Start researcher
//! cargo run -p umao-local-orchestration --bin researcher -- --port 3001
//!
//! # Tab 2: Start synthesizer
//! cargo run -p umao-local-orchestration --bin synthesizer -- --port 3002
//!
//! # Tab 3: Run orchestrator
//! cargo run -p umao-local-orchestration -- --graph graphs/research-pipeline.json
//! ```

use std::sync::Arc;

use clap::Parser;
use tracing::{error, info};

use umao_agents::PromptFleetExecutor;
use umao_core::events::UmaoEvent;
use umao_core::graph::ir::GraphIR;
use umao_executor::orchestrator;

#[derive(Parser)]
#[command(name = "umao-orchestrator", about = "Run a UMAO graph against local A2A agents")]
struct Args {
    /// Path to a graph JSON file.
    #[arg(long)]
    graph: String,

    /// Agent registrations as name=endpoint pairs.
    #[arg(long, value_parser = parse_agent)]
    agent: Vec<(String, String)>,

    /// Default researcher endpoint.
    #[arg(long, default_value = "http://localhost:3001")]
    researcher: String,

    /// Default synthesizer endpoint.
    #[arg(long, default_value = "http://localhost:3002")]
    synthesizer: String,
}

fn parse_agent(s: &str) -> Result<(String, String), String> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        return Err(format!("Expected name=endpoint, got '{}'", s));
    }
    Ok((parts[0].to_string(), parts[1].to_string()))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let graph_json = std::fs::read_to_string(&args.graph).unwrap_or_else(|e| {
        error!(path = %args.graph, error = %e, "Failed to read graph file");
        std::process::exit(1);
    });

    let graph: GraphIR = serde_json::from_str(&graph_json).unwrap_or_else(|e| {
        error!(error = %e, "Failed to parse graph JSON");
        std::process::exit(1);
    });

    info!(
        graph_id = %graph.graph_id,
        nodes = graph.nodes.len(),
        edges = graph.edges.len(),
        "Loaded graph"
    );

    let mut executor = PromptFleetExecutor::new();
    executor.register("researcher", &args.researcher);
    executor.register("synthesizer", &args.synthesizer);
    for (name, endpoint) in &args.agent {
        executor.register(name, endpoint);
    }

    let event_sink: Option<umao_core::events::EventSink> = Some(Arc::new(|event: UmaoEvent| {
        match &event {
            UmaoEvent::NodeStateChanged { fb_id, new_state, .. } => {
                info!(node = %fb_id, state = %new_state, "Node state changed");
            }
            UmaoEvent::NodeCompleted { fb_id, duration_ms, .. } => {
                info!(node = %fb_id, duration_ms, "Node completed");
            }
            UmaoEvent::ExecutionComplete { total_duration_ms, steps_completed, .. } => {
                info!(duration_ms = total_duration_ms, steps = steps_completed, "Execution complete");
            }
            UmaoEvent::NodeFailed { fb_id, error, .. } => {
                error!(node = %fb_id, error, "Node failed");
            }
            _ => {}
        }
    }));

    info!("Starting graph execution...");

    let result = orchestrator::execute_async(&graph, &executor, &event_sink).await;

    match result {
        Ok(exec_result) => {
            info!(
                status = ?exec_result.status,
                budget = exec_result.budget_used,
                steps = exec_result.trace.len(),
                "Orchestration finished"
            );
            println!("\n=== Final Output ===");
            println!(
                "{}",
                serde_json::to_string_pretty(&exec_result.final_output).unwrap_or_default()
            );
        }
        Err(e) => {
            error!(error = %e, "Orchestration failed");
            std::process::exit(1);
        }
    }
}
