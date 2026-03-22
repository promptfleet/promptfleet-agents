#[cfg(all(not(target_arch = "wasm32"), feature = "config-loader", feature = "a2a-server"))]
fn main() -> Result<(), agent_sdk::SdkError> {
    let agent = agent_sdk::AgentBuilder::from_config_path("agent.json")?.build()?;
    let router = agent_sdk::AgentHostBuilder::new(agent)
        .with_a2a()
        .build_router()?;
    let _ = router;
    Ok(())
}

#[cfg(not(all(not(target_arch = "wasm32"), feature = "config-loader", feature = "a2a-server")))]
fn main() {}
