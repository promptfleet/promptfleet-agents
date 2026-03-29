use agent_sdk::{
    Agent,
    a2a::A2aApp,
    agent::{AgentConfig, Response},
};
use spin_sdk::http_component;

static APP: std::sync::OnceLock<A2aApp> = std::sync::OnceLock::new();

#[http_component]
async fn handle_request(req: spin_sdk::http::Request) -> anyhow::Result<spin_sdk::http::Response> {
    let app = APP.get_or_init(|| {
        let config = AgentConfig::new("a2a-echo-wasm", "A2A v1.0 echo agent (WASM)")
            .with_base_url("http://127.0.0.1:3001");
        let mut agent = Agent::new_with_config(config).expect("agent init");
        agent
            .add_skill("echo")
            .description("Echoes back user messages")
            .register()
            .expect("register echo skill");
        agent.set_message_handler(|msg_ctx, task_ctx| async move {
            let input = msg_ctx.text_content.as_deref().unwrap_or("(empty)");
            Response::message_text(
                format!("Echo: {input}"),
                None,
                None,
                task_ctx.and_then(|t| t.context_id),
            )
        });
        A2aApp::from_agent(agent).expect("app init")
    });
    Ok(app.serve_async(req).await?)
}
