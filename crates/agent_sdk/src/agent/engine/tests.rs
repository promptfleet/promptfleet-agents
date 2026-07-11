//! Tests for the ToolEngine — the protocol-agnostic execution core.

#[cfg(not(target_arch = "wasm32"))]
mod tool_engine_tests {
    use crate::agent::engine::types::*;
    use crate::agent::llm_invoker::{LlmStreamFuture, LlmStreamInvoker};
    use crate::agent::tools::{ToolExecutor, ToolKind, ToolRegistry, ToolSpec};
    use crate::agent::trace::AgentTraceEvent;
    use futures::StreamExt;
    use llm_client::LlmError;
    use std::sync::Arc;

    struct MockStreamInvoker {
        turns: std::sync::Mutex<Vec<Vec<llm_client::StreamEvent>>>,
    }

    impl MockStreamInvoker {
        fn new(turns: Vec<Vec<llm_client::StreamEvent>>) -> Self {
            Self {
                turns: std::sync::Mutex::new(turns),
            }
        }
    }

    impl LlmStreamInvoker for MockStreamInvoker {
        fn request_stream(&self, _req: llm_client::LlmRequest) -> LlmStreamFuture {
            let events = {
                let mut guard = self.turns.lock().unwrap();
                if guard.is_empty() {
                    Vec::new()
                } else {
                    guard.remove(0)
                }
            };
            Box::pin(async move {
                let stream = futures::stream::iter(events.into_iter().map(Ok::<_, LlmError>));
                Ok(Box::pin(stream) as llm_client::LlmEventStream)
            })
        }
    }

    fn scenario_invoker(turns: Vec<Vec<llm_client::StreamEvent>>) -> Arc<dyn LlmStreamInvoker> {
        Arc::new(MockStreamInvoker::new(turns))
    }

    fn echo_tools() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        reg.register(ToolSpec {
            name: "echo".to_string(),
            description: Some("Echo the input".to_string()),
            parameters: serde_json::json!({"type":"object","properties":{"text":{"type":"string"}}}),
            kind: ToolKind::Function,
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move { Ok(serde_json::json!({"echoed": args})) })
            })),
        });
        reg
    }

    // =====================================================================
    // run_text tests
    // =====================================================================

    #[tokio::test]
    async fn engine_run_text_simple_response() {
        let invoker = scenario_invoker(vec![vec![
            llm_client::StreamEvent::StreamStart {
                id: Some("r1".into()),
                model: Some("test-model".into()),
            },
            llm_client::StreamEvent::ContentDelta {
                delta: "Hello from engine".into(),
            },
            llm_client::StreamEvent::Done {
                finish_reason: Some("stop".into()),
                usage: Some(llm_client::Usage {
                    prompt_tokens: Some(10),
                    completion_tokens: Some(3),
                    total_tokens: Some(13),
                }),
            },
        ]]);

        let engine = ToolEngine::new(
            invoker,
            "test-model",
            ToolRegistry::new(),
            EngineConfig::default(),
        );

        let result = engine
            .run_text("You are helpful.", "Hi")
            .await
            .expect("should succeed");

        assert_eq!(result.text.as_deref(), Some("Hello from engine"));
        assert_eq!(result.turns_used, 1);
        assert_eq!(result.tool_calls_made, 0);
        assert!(result.usage.is_some());
        assert_eq!(result.usage.unwrap().total_tokens, Some(13));
        assert!(result.stop_signal.is_none());
    }

    #[tokio::test]
    async fn engine_run_text_with_tool_call() {
        let invoker = scenario_invoker(vec![
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_1".into(),
                    name: "echo".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{\"text\":\"world\"}".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ContentDelta {
                    delta: "Echoed!".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let engine = ToolEngine::new(invoker, "test-model", echo_tools(), EngineConfig::default());

        let result = engine
            .run_text("System", "Echo world")
            .await
            .expect("should succeed");

        assert_eq!(result.text.as_deref(), Some("Echoed!"));
        assert_eq!(result.turns_used, 2);
        assert_eq!(result.tool_calls_made, 1);
    }

    #[tokio::test]
    async fn engine_run_text_turn_limit() {
        let invoker = scenario_invoker(vec![
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "c1".into(),
                    name: "echo".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{}".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ContentDelta {
                    delta: "done".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let config = EngineConfig {
            max_turns: Some(1),
            ..Default::default()
        };
        let engine = ToolEngine::new(invoker, "test-model", echo_tools(), config);

        let result = engine.run_text("System", "test").await;
        assert!(result.is_err(), "should fail with turn limit");
        assert!(matches!(result.unwrap_err(), EngineError::TurnLimit { .. }));
    }

    // =====================================================================
    // run_stream tests
    // =====================================================================

    #[tokio::test]
    async fn engine_run_stream_emits_events() {
        let invoker = scenario_invoker(vec![vec![
            llm_client::StreamEvent::StreamStart {
                id: Some("r1".into()),
                model: Some("gpt-4".into()),
            },
            llm_client::StreamEvent::ContentDelta { delta: "Hi".into() },
            llm_client::StreamEvent::ContentDelta {
                delta: " there".into(),
            },
            llm_client::StreamEvent::Done {
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]]);

        let engine = ToolEngine::new(
            invoker,
            "gpt-4",
            ToolRegistry::new(),
            EngineConfig::default(),
        );

        let mut stream = engine.run_stream("System", "Hello");
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        // TurnStarted (from core loop), ContentDelta("Hi"), ContentDelta(" there"),
        // TurnCompleted, Completed
        assert!(events.len() >= 4, "events: {:#?}", events);

        assert!(events.iter().any(|e| matches!(
            e,
            AgentTraceEvent::ContentDelta { delta } if delta == "Hi"
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            AgentTraceEvent::ContentDelta { delta } if delta == " there"
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            AgentTraceEvent::Completed { text, .. } if text.as_deref() == Some("Hi there")
        )));
    }

    // =====================================================================
    // Builder tests
    // =====================================================================

    #[tokio::test]
    async fn engine_builder_works() {
        let invoker: Arc<dyn LlmStreamInvoker> = scenario_invoker(vec![vec![
            llm_client::StreamEvent::StreamStart {
                id: None,
                model: None,
            },
            llm_client::StreamEvent::ContentDelta {
                delta: "Built!".into(),
            },
            llm_client::StreamEvent::Done {
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]]);

        let engine = ToolEngine::builder()
            .llm(invoker)
            .model("test")
            .max_turns(5)
            .timeout_ms(10_000)
            .build()
            .expect("builder should succeed");

        let result = engine
            .run_text("Sys", "User")
            .await
            .expect("should succeed");

        assert_eq!(result.text.as_deref(), Some("Built!"));
    }

    #[test]
    fn engine_builder_requires_llm() {
        let result = ToolEngine::builder().model("test").build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("llm"));
    }

    #[test]
    fn engine_builder_requires_model() {
        let invoker: Arc<dyn LlmStreamInvoker> = scenario_invoker(vec![]);
        let result = ToolEngine::builder().llm(invoker).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("model"));
    }

    // =====================================================================
    // History test
    // =====================================================================

    #[tokio::test]
    async fn engine_run_with_history() {
        let invoker = scenario_invoker(vec![vec![
            llm_client::StreamEvent::StreamStart {
                id: None,
                model: None,
            },
            llm_client::StreamEvent::ContentDelta {
                delta: "Remembering context".into(),
            },
            llm_client::StreamEvent::Done {
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]]);

        let engine = ToolEngine::new(
            invoker,
            "test",
            ToolRegistry::new(),
            EngineConfig::default(),
        );

        let history = &[("user", "What is 2+2?"), ("assistant", "4")];

        let result = engine
            .run_with_history("System", "And 3+3?", history)
            .await
            .expect("should succeed");

        assert_eq!(result.text.as_deref(), Some("Remembering context"));
    }

    // =====================================================================
    // Reasoning deltas test
    // =====================================================================

    #[tokio::test]
    async fn engine_reasoning_deltas_forwarded() {
        let invoker = scenario_invoker(vec![vec![
            llm_client::StreamEvent::StreamStart {
                id: None,
                model: Some("qwen3".into()),
            },
            llm_client::StreamEvent::ReasoningDelta {
                delta: "Thinking...".into(),
            },
            llm_client::StreamEvent::ContentDelta { delta: "42".into() },
            llm_client::StreamEvent::Done {
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]]);

        let engine = ToolEngine::new(
            invoker,
            "qwen3",
            ToolRegistry::new(),
            EngineConfig::default(),
        );

        let mut stream = engine.run_stream("System", "Answer");
        let mut events = Vec::new();
        while let Some(e) = stream.next().await {
            events.push(e);
        }

        let has_reasoning = events.iter().any(
            |e| matches!(e, AgentTraceEvent::ReasoningDelta { delta } if delta == "Thinking..."),
        );
        assert!(
            has_reasoning,
            "expected ReasoningDelta, events: {:#?}",
            events
        );
    }

    // =====================================================================
    // Context-aware tool emits events into trace stream
    // =====================================================================

    #[cfg(feature = "llm-engine")]
    #[tokio::test]
    async fn engine_context_tool_emits_progress_into_stream() {
        use crate::agent::tool_context::ToolContext;

        let mut tools = ToolRegistry::new();
        tools.register(ToolSpec {
            name: "emitter".to_string(),
            description: Some("emits a progress event".to_string()),
            parameters: serde_json::json!({"type":"object","properties":{"text":{"type":"string"}}}),
            kind: ToolKind::Function,
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::WithContext(Arc::new(|_args, ctx| {
                Box::pin(async move {
                    ctx.emit(AgentTraceEvent::ProgressUpdate {
                        message: "sub-agent working...".to_string(),
                        progress_pct: Some(50),
                        metadata: None,
                    });
                    Ok(serde_json::json!({"status": "ok"}))
                })
            })),
        });

        let invoker = scenario_invoker(vec![
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "c1".into(),
                    name: "emitter".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{\"text\":\"go\"}".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ContentDelta {
                    delta: "done".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let engine = ToolEngine::new(invoker, "test", tools, EngineConfig::default());
        let mut stream = engine.run_stream("You are a test agent", "go");
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        let progress = events.iter().find(|e| {
            matches!(e, AgentTraceEvent::ProgressUpdate { message, .. } if message == "sub-agent working...")
        });
        assert!(
            progress.is_some(),
            "ProgressUpdate emitted by context-aware tool must appear in trace stream, got: {:?}",
            events
                .iter()
                .map(|e| format!("{:?}", std::mem::discriminant(e)))
                .collect::<Vec<_>>()
        );
    }

    // =====================================================================
    // Failed tool recovery
    // =====================================================================

    #[tokio::test]
    async fn engine_failed_tool_recovery() {
        let invoker = scenario_invoker(vec![
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ToolCallStart {
                    index: 0,
                    id: "c1".into(),
                    name: "nonexistent".into(),
                },
                llm_client::StreamEvent::ToolCallDelta {
                    index: 0,
                    arguments_delta: "{}".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("tool_calls".into()),
                    usage: None,
                },
            ],
            vec![
                llm_client::StreamEvent::StreamStart {
                    id: None,
                    model: None,
                },
                llm_client::StreamEvent::ContentDelta {
                    delta: "Recovered".into(),
                },
                llm_client::StreamEvent::Done {
                    finish_reason: Some("stop".into()),
                    usage: None,
                },
            ],
        ]);

        let engine = ToolEngine::new(invoker, "test", echo_tools(), EngineConfig::default());

        let result = engine
            .run_text("System", "call nonexistent")
            .await
            .expect("should recover");

        assert_eq!(result.text.as_deref(), Some("Recovered"));
        assert_eq!(result.tool_calls_made, 1);
    }

    // =====================================================================
    // No tools (text-only)
    // =====================================================================

    #[tokio::test]
    async fn engine_no_tools_text_only() {
        let invoker = scenario_invoker(vec![vec![
            llm_client::StreamEvent::StreamStart {
                id: None,
                model: None,
            },
            llm_client::StreamEvent::ContentDelta {
                delta: "Just text".into(),
            },
            llm_client::StreamEvent::Done {
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]]);

        let engine = ToolEngine::new(
            invoker,
            "test",
            ToolRegistry::new(),
            EngineConfig::default(),
        );

        let result = engine
            .run_text("System", "Hello")
            .await
            .expect("should succeed");

        assert_eq!(result.text.as_deref(), Some("Just text"));
        assert_eq!(result.tool_calls_made, 0);
    }

    // =====================================================================
    // Stop signal (sentinel tool)
    // =====================================================================

    #[tokio::test]
    async fn engine_stop_signal_from_sentinel_tool() {
        let invoker = scenario_invoker(vec![vec![
            llm_client::StreamEvent::StreamStart {
                id: None,
                model: None,
            },
            llm_client::StreamEvent::ToolCallStart {
                index: 0,
                id: "call_ck".into(),
                name: "checkpoint_task".into(),
            },
            llm_client::StreamEvent::ToolCallDelta {
                index: 0,
                arguments_delta: "{\"task_patch\":{\"state\":\"completed\"}}".into(),
            },
            llm_client::StreamEvent::Done {
                finish_reason: Some("tool_calls".into()),
                usage: None,
            },
        ]]);

        let mut tools = ToolRegistry::new();
        tools.register(ToolSpec {
            name: "checkpoint_task".to_string(),
            description: Some("Sentinel tool".to_string()),
            parameters: serde_json::json!({"type":"object"}),
            kind: ToolKind::Function,
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move {
                    Ok(serde_json::json!({
                        "__engine_stop": true,
                        "__checkpoint_args": args,
                    }))
                })
            })),
        });

        let engine = ToolEngine::new(invoker, "test", tools, EngineConfig::default());

        let result = engine
            .run_text("System", "Do something")
            .await
            .expect("should succeed with stop signal");

        assert!(result.stop_signal.is_some());
        let signal = result.stop_signal.unwrap();
        assert_eq!(
            signal.get("__engine_stop").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(signal.get("__checkpoint_args").is_some());
        assert_eq!(result.tool_calls_made, 1);
    }
}

// =========================================================================
// Core loop tests (direct, no ToolEngine wrapper)
// =========================================================================

#[cfg(not(target_arch = "wasm32"))]
mod core_loop_tests {
    use crate::agent::engine::core_loop;
    use crate::agent::engine::types::*;
    use crate::agent::tools::{ToolExecutor, ToolKind, ToolRegistry, ToolSpec};
    use crate::agent::trace::AgentTraceEvent;
    use llm_client::ChatMessage;
    use llm_client::LlmRequest;
    use std::sync::{Arc, Mutex};

    struct MockTurnInvoker {
        turns: Mutex<Vec<TurnResult>>,
        requests: Mutex<Vec<LlmRequest>>,
    }

    impl MockTurnInvoker {
        fn new(turns: Vec<TurnResult>) -> Self {
            Self {
                turns: Mutex::new(turns),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl LlmTurnInvoker for MockTurnInvoker {
        fn invoke_turn(&self, request: LlmRequest) -> TurnFuture {
            self.requests.lock().unwrap().push(request);
            let result = {
                let mut guard = self.turns.lock().unwrap();
                if guard.is_empty() {
                    TurnResult {
                        content: String::new(),
                        tool_calls: vec![],
                        finish_reason: Some("stop".into()),
                        usage: None,
                    }
                } else {
                    guard.remove(0)
                }
            };
            Box::pin(async move { Ok(result) })
        }
    }

    fn echo_tools() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        reg.register(ToolSpec {
            name: "echo".to_string(),
            description: Some("Echo".to_string()),
            parameters: serde_json::json!({"type":"object","properties":{"text":{"type":"string"}}}),
            kind: ToolKind::Function,
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move { Ok(serde_json::json!({"echoed": args})) })
            })),
        });
        reg
    }

    #[tokio::test]
    async fn core_loop_simple_text() {
        let invoker = MockTurnInvoker::new(vec![TurnResult {
            content: "Hello".into(),
            tool_calls: vec![],
            finish_reason: Some("stop".into()),
            usage: Some(llm_client::Usage {
                prompt_tokens: Some(5),
                completion_tokens: Some(1),
                total_tokens: Some(6),
            }),
        }]);

        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let on_event = move |e: AgentTraceEvent| {
            ev.lock().unwrap().push(e);
        };

        let mut messages = vec![ChatMessage {
            role: "user".into(),
            content: Some("Hi".into()),
            ..Default::default()
        }];
        let result = core_loop::execute(
            &invoker,
            "test",
            &ToolRegistry::new(),
            &EngineConfig::default(),
            &mut messages,
            &on_event,
            None,
            None,
            None,
        )
        .await
        .expect("should succeed");

        assert_eq!(result.text.as_deref(), Some("Hello"));
        assert_eq!(result.turns_used, 1);
        assert_eq!(result.tool_calls_made, 0);
        assert!(result.stop_signal.is_none());

        let ev = events.lock().unwrap();
        assert!(
            ev.iter()
                .any(|e| matches!(e, AgentTraceEvent::TurnStarted { turn: 1, .. }))
        );
        assert!(
            ev.iter()
                .any(|e| matches!(e, AgentTraceEvent::TurnCompleted { turn: 1, .. }))
        );
        assert!(
            ev.iter()
                .any(|e| matches!(e, AgentTraceEvent::Completed { .. }))
        );
    }

    #[tokio::test]
    async fn core_loop_tool_call_then_text_preserves_second_turn_history() {
        let invoker = MockTurnInvoker::new(vec![
            TurnResult {
                content: String::new(),
                tool_calls: vec![ToolCallInfo {
                    index: 0,
                    id: "call_1".into(),
                    name: "echo".into(),
                    arguments_raw: "{\"text\":\"hello\"}".into(),
                }],
                finish_reason: Some("tool_calls".into()),
                usage: None,
            },
            TurnResult {
                content: "Done!".into(),
                tool_calls: vec![],
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]);

        let mut messages = vec![ChatMessage {
            role: "user".into(),
            content: Some("echo hello".into()),
            ..Default::default()
        }];
        let result = core_loop::execute(
            &invoker,
            "test",
            &echo_tools(),
            &EngineConfig::default(),
            &mut messages,
            &|_| {},
            None,
            None,
            None,
        )
        .await
        .expect("should succeed");

        assert_eq!(result.text.as_deref(), Some("Done!"));
        assert_eq!(result.turns_used, 2);
        assert_eq!(result.tool_calls_made, 1);

        let requests = invoker.requests.lock().unwrap();
        let history = &requests[1].messages;
        assert_eq!(history.len(), 3);
        assert_eq!(history[1].role, "assistant");
        let tool_call = &history[1].tool_calls.as_ref().expect("tool calls")[0];
        assert_eq!(tool_call.id, "call_1");
        assert_eq!(tool_call.name, "echo");
        assert_eq!(tool_call.arguments, serde_json::json!({"text": "hello"}));
        assert_eq!(history[2].role, "tool");
        assert_eq!(history[2].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(
            history[2].content.as_deref(),
            Some("{\"echoed\":{\"text\":\"hello\"}}")
        );
    }

    #[tokio::test]
    async fn core_loop_turn_limit() {
        let invoker = MockTurnInvoker::new(vec![
            TurnResult {
                content: String::new(),
                tool_calls: vec![ToolCallInfo {
                    index: 0,
                    id: "c1".into(),
                    name: "echo".into(),
                    arguments_raw: "{}".into(),
                }],
                finish_reason: Some("tool_calls".into()),
                usage: None,
            },
            TurnResult {
                content: "should not reach".into(),
                tool_calls: vec![],
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]);

        let config = EngineConfig {
            max_turns: Some(1),
            ..Default::default()
        };

        let mut messages = vec![ChatMessage {
            role: "user".into(),
            content: Some("test".into()),
            ..Default::default()
        }];
        let result = core_loop::execute(
            &invoker,
            "test",
            &echo_tools(),
            &config,
            &mut messages,
            &|_| {},
            None,
            None,
            None,
        )
        .await;

        assert!(matches!(result, Err(EngineError::TurnLimit { .. })));
    }

    #[tokio::test]
    async fn core_loop_none_turn_limit_allows_more_turns() {
        let invoker = MockTurnInvoker::new(vec![
            TurnResult {
                content: String::new(),
                tool_calls: vec![ToolCallInfo {
                    index: 0,
                    id: "c1".into(),
                    name: "echo".into(),
                    arguments_raw: "{}".into(),
                }],
                finish_reason: Some("tool_calls".into()),
                usage: None,
            },
            TurnResult {
                content: String::new(),
                tool_calls: vec![ToolCallInfo {
                    index: 0,
                    id: "c2".into(),
                    name: "echo".into(),
                    arguments_raw: "{}".into(),
                }],
                finish_reason: Some("tool_calls".into()),
                usage: None,
            },
            TurnResult {
                content: "done".into(),
                tool_calls: vec![],
                finish_reason: Some("stop".into()),
                usage: None,
            },
        ]);

        let config = EngineConfig {
            max_turns: None,
            ..Default::default()
        };

        let mut messages = vec![ChatMessage {
            role: "user".into(),
            content: Some("test".into()),
            ..Default::default()
        }];
        let result = core_loop::execute(
            &invoker,
            "test",
            &echo_tools(),
            &config,
            &mut messages,
            &|_| {},
            None,
            None,
            None,
        )
        .await
        .expect("unlimited turn limit should allow completion");

        assert_eq!(result.text.as_deref(), Some("done"));
        assert_eq!(result.turns_used, 3);
        assert_eq!(result.tool_calls_made, 2);
    }

    #[tokio::test]
    async fn core_loop_stop_signal() {
        let invoker = MockTurnInvoker::new(vec![TurnResult {
            content: String::new(),
            tool_calls: vec![ToolCallInfo {
                index: 0,
                id: "call_ck".into(),
                name: "sentinel".into(),
                arguments_raw: "{\"data\":\"test\"}".into(),
            }],
            finish_reason: Some("tool_calls".into()),
            usage: None,
        }]);

        let mut tools = ToolRegistry::new();
        tools.register(ToolSpec {
            name: "sentinel".to_string(),
            description: Some("Sentinel".to_string()),
            parameters: serde_json::json!({"type":"object"}),
            kind: ToolKind::Function,
            strict: false,
            parallel_ok: false,
            executor: ToolExecutor::Simple(Arc::new(|args| {
                Box::pin(async move {
                    Ok(serde_json::json!({
                        "__engine_stop": true,
                        "__checkpoint_args": args,
                    }))
                })
            })),
        });

        let mut messages = vec![ChatMessage {
            role: "user".into(),
            content: Some("stop".into()),
            ..Default::default()
        }];
        let result = core_loop::execute(
            &invoker,
            "test",
            &tools,
            &EngineConfig::default(),
            &mut messages,
            &|_| {},
            None,
            None,
            None,
        )
        .await
        .expect("should succeed");

        assert!(result.stop_signal.is_some());
        let signal = result.stop_signal.unwrap();
        assert_eq!(
            signal
                .get("__checkpoint_args")
                .and_then(|v| v.get("data"))
                .and_then(|v| v.as_str()),
            Some("test")
        );
    }

    #[tokio::test]
    async fn core_loop_messages_preserved_on_error() {
        let invoker = MockTurnInvoker::new(vec![TurnResult {
            content: String::new(),
            tool_calls: vec![ToolCallInfo {
                index: 0,
                id: "c1".into(),
                name: "echo".into(),
                arguments_raw: "{}".into(),
            }],
            finish_reason: Some("tool_calls".into()),
            usage: None,
        }]);

        let config = EngineConfig {
            max_turns: Some(1),
            ..Default::default()
        };

        let mut messages = vec![ChatMessage {
            role: "user".into(),
            content: Some("test".into()),
            ..Default::default()
        }];
        let _ = core_loop::execute(
            &invoker,
            "test",
            &echo_tools(),
            &config,
            &mut messages,
            &|_| {},
            None,
            None,
            None,
        )
        .await;

        // Messages should contain: original user + assistant tool_calls + tool result
        assert!(
            messages.len() > 1,
            "messages should be preserved for adapter use: {:?}",
            messages
        );
    }
}
