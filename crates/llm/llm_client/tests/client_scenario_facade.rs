//! [`LlmClient`] against [`pf_test_harness::scenario::LlmScenario`] via a local OpenAI-compatible mock.

#![cfg(not(target_arch = "wasm32"))]

use futures::StreamExt;
use llm_client::auth::ApiKeyAuth;
use llm_client::client::{LlmClient, WireFormat};
use llm_client::model_client::ApiMode;
use llm_client::StreamEvent;
use llm_client::{ChatMessage, LlmRequest};
use pf_test_harness::scenario::LlmScenario;
use pf_test_harness::scenario_openai_http::OpenAiScenarioMock;

fn user_msg(text: &str) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: Some(text.into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn llm_client_chat_uses_scenario_mock() {
    let scenario = LlmScenario::single_text("scenario-hello");
    let mock = OpenAiScenarioMock::new(scenario);
    let (base, _h) = mock.spawn().await;

    let client = LlmClient::builder(WireFormat::OpenAiCompat)
        .base_url(&base)
        .auth(ApiKeyAuth::new("sk-mock"))
        .api_mode(ApiMode::Chat)
        .build()
        .expect("client");

    let resp = client
        .chat(LlmRequest {
            model: "mock-model".into(),
            messages: vec![user_msg("hi")],
            max_tokens: Some(16),
            ..Default::default()
        })
        .await
        .expect("chat");

    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("scenario-hello")
    );
}

#[tokio::test]
async fn llm_client_chat_stream_uses_scenario_mock() {
    let scenario = LlmScenario::single_text("stream-hello");
    let mock = OpenAiScenarioMock::new(scenario);
    let (base, _h) = mock.spawn().await;

    let client = LlmClient::builder(WireFormat::OpenAiCompat)
        .base_url(&base)
        .auth(ApiKeyAuth::new("sk-mock"))
        .api_mode(ApiMode::Chat)
        .build()
        .expect("client");

    let mut stream = client
        .chat_stream(LlmRequest {
            model: "m".into(),
            messages: vec![user_msg("x")],
            ..Default::default()
        })
        .await
        .expect("chat_stream");

    let mut text = String::new();
    while let Some(ev) = stream.next().await {
        match ev.expect("event") {
            StreamEvent::ContentDelta { delta } => text.push_str(&delta),
            _ => {}
        }
    }
    assert_eq!(text, "stream-hello");
}
