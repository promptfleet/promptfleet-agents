#[cfg(feature = "config-loader")]
fn main() -> Result<(), agent_sdk::SdkError> {
    let agent = agent_sdk::AgentBuilder::from_config_path("agent.json")?.build()?;
    let _ = agent;
    Ok(())
}

#[cfg(not(feature = "config-loader"))]
fn main() {}
