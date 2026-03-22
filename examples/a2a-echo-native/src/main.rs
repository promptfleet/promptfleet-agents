use agent_sdk::{
    a2a::A2aApp,
    agent::{AgentConfig, MessageContext, Response, TaskContext},
    Agent,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut config = AgentConfig::new("a2a-echo-native", "A2A v1.0 echo agent (native)")
        .with_base_url("http://127.0.0.1:3000");
    config.streaming = true;

    let mut agent = Agent::new_with_config(config)?;
    agent.add_skill("echo", "Echoes back user messages");
    agent.set_message_handler(echo_handler);

    let app = A2aApp::from_agent(agent)?;
    println!("A2A echo agent (native) listening on http://127.0.0.1:3000");
    app.serve("127.0.0.1:3000").await
}

async fn echo_handler(
    msg_ctx: MessageContext,
    task_ctx: Option<TaskContext>,
) -> agent_sdk::SdkResult<agent_sdk::agent::RuntimeResponse> {
    let input = msg_ctx.text_content.as_deref().unwrap_or("(empty)");
    Response::message_text(
        format!("Echo: {input}"),
        None,
        None,
        task_ctx.and_then(|t| t.context_id),
    )
}
