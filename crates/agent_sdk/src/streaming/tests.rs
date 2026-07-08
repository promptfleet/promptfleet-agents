#[cfg(test)]
mod agent_io_serialization {
    use crate::streaming::events::AgentIoEvent;
    use serde_json::json;

    fn wire_type(event: &AgentIoEvent) -> &'static str {
        event.wire_type()
    }

    #[test]
    fn run_started_wire_name() {
        let e = AgentIoEvent::RunStarted {
            thread_id: "t1".into(),
            run_id: "r1".into(),
        };
        assert_eq!(wire_type(&e), "RUN_STARTED");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["threadId"], "t1");
        assert_eq!(v["runId"], "r1");
    }

    #[test]
    fn run_finished_wire_name() {
        let e = AgentIoEvent::RunFinished {
            thread_id: "t1".into(),
            run_id: "r1".into(),
            result: Some(json!({"usage": {"total_tokens": 42}})),
        };
        assert_eq!(wire_type(&e), "RUN_FINISHED");
    }

    #[test]
    fn run_error_wire_name() {
        let e = AgentIoEvent::RunError {
            message: "boom".into(),
            code: Some("E1".into()),
        };
        assert_eq!(wire_type(&e), "RUN_ERROR");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["message"], "boom");
        assert_eq!(v["code"], "E1");
    }

    #[test]
    fn run_error_without_code_omits_field() {
        let e = AgentIoEvent::RunError {
            message: "x".into(),
            code: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert!(v.get("code").is_none());
    }

    #[test]
    fn step_started_wire_name() {
        let e = AgentIoEvent::StepStarted {
            step_name: "turn_1".into(),
        };
        assert_eq!(wire_type(&e), "STEP_STARTED");
    }

    #[test]
    fn step_finished_wire_name() {
        let e = AgentIoEvent::StepFinished {
            step_name: "turn_2".into(),
        };
        assert_eq!(wire_type(&e), "STEP_FINISHED");
    }

    #[test]
    fn text_message_start_wire_name() {
        let e = AgentIoEvent::TextMessageStart {
            message_id: "m1".into(),
            role: "assistant".into(),
        };
        assert_eq!(wire_type(&e), "TEXT_MESSAGE_START");
    }

    #[test]
    fn text_message_content_wire_name() {
        let e = AgentIoEvent::TextMessageContent {
            message_id: "m1".into(),
            delta: "hello".into(),
        };
        assert_eq!(wire_type(&e), "TEXT_MESSAGE_CONTENT");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["delta"], "hello");
    }

    #[test]
    fn text_message_end_wire_name() {
        let e = AgentIoEvent::TextMessageEnd {
            message_id: "m1".into(),
        };
        assert_eq!(wire_type(&e), "TEXT_MESSAGE_END");
    }

    #[test]
    fn reasoning_lifecycle_wire_names() {
        assert_eq!(
            wire_type(&AgentIoEvent::ReasoningStart {
                message_id: "r".into()
            }),
            "REASONING_START"
        );
        assert_eq!(
            wire_type(&AgentIoEvent::ReasoningMessageStart {
                message_id: "r".into(),
                role: "assistant".into()
            }),
            "REASONING_MESSAGE_START"
        );
        assert_eq!(
            wire_type(&AgentIoEvent::ReasoningMessageContent {
                message_id: "r".into(),
                delta: "think".into()
            }),
            "REASONING_MESSAGE_CONTENT"
        );
        assert_eq!(
            wire_type(&AgentIoEvent::ReasoningMessageEnd {
                message_id: "r".into()
            }),
            "REASONING_MESSAGE_END"
        );
        assert_eq!(
            wire_type(&AgentIoEvent::ReasoningEnd {
                message_id: "r".into()
            }),
            "REASONING_END"
        );
    }

    #[test]
    fn tool_call_lifecycle_wire_names() {
        assert_eq!(
            wire_type(&AgentIoEvent::ToolCallStart {
                tool_call_id: "c1".into(),
                tool_call_name: "search".into(),
                parent_message_id: None,
            }),
            "TOOL_CALL_START"
        );
        assert_eq!(
            wire_type(&AgentIoEvent::ToolCallArgs {
                tool_call_id: "c1".into(),
                delta: "{\"q\":".into(),
            }),
            "TOOL_CALL_ARGS"
        );
        assert_eq!(
            wire_type(&AgentIoEvent::ToolCallEnd {
                tool_call_id: "c1".into(),
            }),
            "TOOL_CALL_END"
        );
        assert_eq!(
            wire_type(&AgentIoEvent::ToolCallResult {
                tool_call_id: "c1".into(),
                message_id: None,
                content: json!("ok"),
                role: Some("tool".into()),
            }),
            "TOOL_CALL_RESULT"
        );
    }

    #[test]
    fn pf_extensions_use_custom_wire_type() {
        let event = AgentIoEvent::Custom {
            name: "delegation_started".into(),
            value: json!({
                "delegationId": "d1",
                "toolCallId": "c1",
                "subagent": "planner",
            }),
        };
        assert_eq!(wire_type(&event), "CUSTOM");
        let v = serde_json::to_value(&event).unwrap();
        assert_eq!(v["type"], "CUSTOM");
        assert_eq!(v["name"], "delegation_started");
        assert_eq!(v["value"]["toolCallId"], "c1");
    }

    #[test]
    fn tool_call_start_omits_none_parent() {
        let e = AgentIoEvent::ToolCallStart {
            tool_call_id: "c1".into(),
            tool_call_name: "fn".into(),
            parent_message_id: None,
        };
        let v = serde_json::to_value(&e).unwrap();
        assert!(v.get("parentMessageId").is_none());
    }

    #[test]
    fn custom_wire_name() {
        let e = AgentIoEvent::Custom {
            name: "citations_updated".into(),
            value: json!({"items": []}),
        };
        assert_eq!(wire_type(&e), "CUSTOM");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["name"], "citations_updated");
    }
}

#[cfg(test)]
mod agent_io_mapper {
    use crate::agent::trace::AgentTraceEvent;
    use crate::streaming::events::IoEventContext;
    use crate::streaming::mapper::map_trace_to_agent_io;
    use serde_json::json;

    fn ctx() -> IoEventContext {
        IoEventContext {
            thread_id: "thread-1".into(),
            run_id: "run-1".into(),
            message_id: "msg-1".into(),
        }
    }

    fn wire_types(events: &[crate::streaming::events::AgentIoEvent]) -> Vec<String> {
        events
            .iter()
            .map(|e| {
                let v = serde_json::to_value(e).unwrap();
                v["type"].as_str().unwrap().to_string()
            })
            .collect()
    }

    #[test]
    fn turn_started_maps_to_step_started() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::TurnStarted {
                turn: 1,
                response_id: None,
                model: None,
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["STEP_STARTED"]);
    }

    #[test]
    fn content_delta_maps_to_text_message_content() {
        let out =
            map_trace_to_agent_io(AgentTraceEvent::ContentDelta { delta: "hi".into() }, &ctx());
        assert_eq!(wire_types(&out), vec!["TEXT_MESSAGE_CONTENT"]);
        let v = serde_json::to_value(&out[0]).unwrap();
        assert_eq!(v["delta"], "hi");
        assert_eq!(v["messageId"], "msg-1");
    }

    #[test]
    fn reasoning_started_maps_to_start_plus_message_start() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ReasoningStarted {
                message_id: "r-1".into(),
            },
            &ctx(),
        );
        assert_eq!(
            wire_types(&out),
            vec!["REASONING_START", "REASONING_MESSAGE_START"]
        );
    }

    #[test]
    fn reasoning_delta_maps_to_reasoning_message_content() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ReasoningDelta {
                delta: "hmm".into(),
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["REASONING_MESSAGE_CONTENT"]);
    }

    #[test]
    fn reasoning_completed_maps_to_message_end_plus_end() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ReasoningCompleted {
                message_id: "r-1".into(),
            },
            &ctx(),
        );
        assert_eq!(
            wire_types(&out),
            vec!["REASONING_MESSAGE_END", "REASONING_END"]
        );
    }

    #[test]
    fn tool_call_started_maps_to_tool_call_start() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ToolCallStarted {
                index: 0,
                id: "call_1".into(),
                name: "search".into(),
                arguments: serde_json::Value::Null,
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["TOOL_CALL_START"]);
        let v = serde_json::to_value(&out[0]).unwrap();
        assert_eq!(v["toolCallId"], "call_1");
        assert_eq!(v["toolCallName"], "search");
        assert_eq!(v["parentMessageId"], "msg-1");
    }

    #[test]
    fn tool_call_args_delta_maps_to_tool_call_args() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ToolCallArgsDelta {
                index: 0,
                id: "call_1".into(),
                delta: "{\"q\":".into(),
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["TOOL_CALL_ARGS"]);
    }

    #[test]
    fn tool_call_args_completed_maps_to_tool_call_end() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ToolCallArgsCompleted {
                index: 0,
                id: "call_1".into(),
                name: "search".into(),
                arguments: json!({"q": "rust"}),
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["TOOL_CALL_END"]);
    }

    #[test]
    fn tool_call_completed_maps_to_tool_call_result() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ToolCallCompleted {
                index: 0,
                id: "call_1".into(),
                name: "search".into(),
                result: json!({"data": "found"}),
                duration_ms: 150,
                success: true,
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["TOOL_CALL_RESULT"]);
        let v = serde_json::to_value(&out[0]).unwrap();
        assert_eq!(v["content"]["data"], "found");
    }

    #[test]
    fn turn_completed_maps_to_step_finished() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::TurnCompleted {
                turn: 1,
                finish_reason: Some("stop".into()),
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["STEP_FINISHED"]);
    }

    #[test]
    fn completed_maps_to_run_finished_with_usage() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::Completed {
                text: Some("done".into()),
                usage: Some(::llm_client::Usage {
                    prompt_tokens: Some(10),
                    completion_tokens: Some(5),
                    total_tokens: Some(15),
                }),
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["RUN_FINISHED"]);
        let v = serde_json::to_value(&out[0]).unwrap();
        assert_eq!(v["threadId"], "thread-1");
        assert_eq!(v["runId"], "run-1");
    }

    #[test]
    fn progress_update_maps_to_custom() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ProgressUpdate {
                message: "Searching...".into(),
                progress_pct: Some(50),
                metadata: None,
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["CUSTOM"]);
        let v = serde_json::to_value(&out[0]).unwrap();
        assert_eq!(v["name"], "progress_update");
    }

    #[test]
    fn subagent_progress_maps_to_delegation_progress() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ProgressUpdate {
                message: "planner: build graph".into(),
                progress_pct: None,
                metadata: Some(json!({
                    "source": "sub_agent",
                    "delegation_id": "call_1",
                    "tool_call_id": "call_1",
                    "agent_name": "planner",
                    "task_id": "task-1",
                    "state": "working",
                })),
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["CUSTOM"]);
        let v = serde_json::to_value(&out[0]).unwrap();
        assert_eq!(v["name"], "delegation_progress");
        assert_eq!(v["value"]["subagent"], "planner");
        assert_eq!(v["value"]["toolCallId"], "call_1");
    }

    #[test]
    fn subagent_handoff_maps_to_delegation_started() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::AgentHandoff {
                from_agent: "self".into(),
                to_agent: "planner".into(),
                metadata: Some(json!({
                    "source": "sub_agent",
                    "phase": "start",
                    "delegation_id": "call_1",
                    "tool_call_id": "call_1",
                    "agent_name": "planner",
                    "task_id": "task-1",
                    "mode": "streaming",
                })),
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["CUSTOM"]);
    }

    #[test]
    fn subagent_tool_completion_maps_to_delegation_finished_then_tool_result() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ToolCallCompleted {
                index: 0,
                id: "call_1".into(),
                name: "umao_plan".into(),
                result: json!({
                    "delegation_id": "call_1",
                    "tool_call_id": "call_1",
                    "agent_name": "planner",
                    "task_id": "task-1",
                    "result": {"graph": {"version": 1}}
                }),
                duration_ms: 42,
                success: true,
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["CUSTOM", "TOOL_CALL_RESULT"]);
    }

    #[test]
    fn subagent_tool_failure_maps_to_delegation_failed_then_tool_result() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ToolCallCompleted {
                index: 0,
                id: "call_1".into(),
                name: "umao_execute".into(),
                result: json!({
                    "error": {
                        "source": "sub_agent",
                        "delegation_id": "call_1",
                        "tool_call_id": "call_1",
                        "agent_name": "executor",
                        "task_id": "task-1",
                        "error_kind": "broken_stream",
                        "message": "SSE read failed"
                    }
                }),
                duration_ms: 42,
                success: false,
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["CUSTOM", "TOOL_CALL_RESULT"]);
    }

    #[test]
    fn context_trimmed_maps_to_custom() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::ContextTrimmed {
                evicted_count: 3,
                remaining_count: 10,
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["CUSTOM"]);
        let v = serde_json::to_value(&out[0]).unwrap();
        assert_eq!(v["name"], "context_summarized");
    }

    #[test]
    fn failed_maps_to_run_error() {
        let out = map_trace_to_agent_io(
            AgentTraceEvent::Failed {
                message: "timeout".into(),
            },
            &ctx(),
        );
        assert_eq!(wire_types(&out), vec!["RUN_ERROR"]);
        let v = serde_json::to_value(&out[0]).unwrap();
        assert_eq!(v["message"], "timeout");
        assert_eq!(v["code"], "ENGINE_ERROR");
    }

    #[test]
    fn full_tool_call_ag_ui_sequence() {
        let c = ctx();
        let mut all = Vec::new();
        all.extend(map_trace_to_agent_io(
            AgentTraceEvent::ToolCallStarted {
                index: 0,
                id: "c1".into(),
                name: "search".into(),
                arguments: serde_json::Value::Null,
            },
            &c,
        ));
        all.extend(map_trace_to_agent_io(
            AgentTraceEvent::ToolCallArgsDelta {
                index: 0,
                id: "c1".into(),
                delta: "{\"q\":\"rust\"}".into(),
            },
            &c,
        ));
        all.extend(map_trace_to_agent_io(
            AgentTraceEvent::ToolCallArgsCompleted {
                index: 0,
                id: "c1".into(),
                name: "search".into(),
                arguments: json!({"q": "rust"}),
            },
            &c,
        ));
        all.extend(map_trace_to_agent_io(
            AgentTraceEvent::ToolCallCompleted {
                index: 0,
                id: "c1".into(),
                name: "search".into(),
                result: json!("found"),
                duration_ms: 100,
                success: true,
            },
            &c,
        ));
        assert_eq!(
            wire_types(&all),
            vec![
                "TOOL_CALL_START",
                "TOOL_CALL_ARGS",
                "TOOL_CALL_END",
                "TOOL_CALL_RESULT"
            ]
        );
    }
}

#[cfg(test)]
mod a2a_sse_tests {
    use a2a_protocol_core::data::{TaskState, TaskStatus};
    use a2a_protocol_core::streaming::{
        StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent,
    };
    use serde_json::json;

    #[test]
    fn task_status_update_event_name() {
        let e = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            id: json!("req-1"),
            task_id: "t-1".into(),
            context_id: "ctx-1".into(),
            status: TaskStatus::new(TaskState::Working),
        });
        assert_eq!(e.event_name(), "statusUpdate");
    }

    #[test]
    fn task_artifact_update_event_name() {
        let artifact = a2a_protocol_core::data::artifact::Artifact::text("hello");
        let e = StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            id: json!("req-2"),
            task_id: "t-1".into(),
            context_id: "ctx-1".into(),
            artifact,
            append: None,
            last_chunk: Some(true),
        });
        assert_eq!(e.event_name(), "artifactUpdate");
    }

    #[test]
    fn task_status_update_jsonrpc_envelope() {
        let e = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            id: json!("req-1"),
            task_id: "task-42".into(),
            context_id: "ctx-1".into(),
            status: TaskStatus::new(TaskState::Completed),
        });
        let data = e.to_jsonrpc_data();
        assert_eq!(data["jsonrpc"], "2.0");
        assert_eq!(data["id"], "req-1");
    }

    #[test]
    fn task_status_update_with_message() {
        use a2a_protocol_core::data::{Message, MessageRole, Part};
        let msg = Message::new(
            MessageRole::Agent,
            vec![Part::text("hello world")],
            "task-1".into(),
        );
        let e = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            id: json!("r1"),
            task_id: "task-1".into(),
            context_id: "ctx-1".into(),
            status: TaskStatus {
                state: TaskState::Working,
                message: Some(msg),
                timestamp: None,
            },
        });
        let data = e.to_jsonrpc_data();
        let status_update = &data["result"]["statusUpdate"];
        assert!(status_update["status"].is_object());
    }

    #[test]
    fn task_artifact_update_jsonrpc_envelope() {
        let artifact = a2a_protocol_core::data::artifact::Artifact::text("content");
        let e = StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            id: json!(42),
            task_id: "task-7".into(),
            context_id: "ctx-7".into(),
            artifact,
            append: None,
            last_chunk: Some(false),
        });
        let data = e.to_jsonrpc_data();
        assert_eq!(data["jsonrpc"], "2.0");
        assert_eq!(data["id"], 42);
    }

    #[test]
    fn serde_roundtrip() {
        let original = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            id: json!("r1"),
            task_id: "t1".into(),
            context_id: "ctx-1".into(),
            status: TaskStatus::new(TaskState::Failed),
        });
        let json_str = serde_json::to_string(&original).unwrap();
        let parsed: StreamResponse = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.event_name(), "statusUpdate");
    }
}

#[cfg(test)]
mod a2a_sse_mapper {
    use crate::agent::trace::AgentTraceEvent;
    use crate::streaming::mapper::{A2aSseContext, map_trace_to_stream_response};
    use a2a_protocol_core::streaming::StreamResponse;
    use serde_json::json;

    fn ctx() -> A2aSseContext {
        A2aSseContext {
            task_id: "task-1".into(),
            context_id: "ctx-1".into(),
            jsonrpc_id: json!("req-1"),
        }
    }

    #[test]
    fn turn_started_maps_to_working_status() {
        let out = map_trace_to_stream_response(
            AgentTraceEvent::TurnStarted {
                turn: 1,
                response_id: None,
                model: None,
            },
            &ctx(),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].event_name(), "statusUpdate");
        match &out[0] {
            StreamResponse::StatusUpdate(ev) => {
                assert_eq!(ev.status.state, a2a_protocol_core::data::TaskState::Working);
            }
            _ => panic!("expected StatusUpdate"),
        }
    }

    #[test]
    fn content_delta_maps_to_working_with_message() {
        let out = map_trace_to_stream_response(
            AgentTraceEvent::ContentDelta {
                delta: "hello".into(),
            },
            &ctx(),
        );
        assert_eq!(out.len(), 1);
        match &out[0] {
            StreamResponse::StatusUpdate(ev) => {
                assert_eq!(ev.status.state, a2a_protocol_core::data::TaskState::Working);
                assert!(ev.status.message.is_some());
            }
            _ => panic!("expected StatusUpdate"),
        }
    }

    #[test]
    fn completed_maps_to_terminal_completed() {
        let out = map_trace_to_stream_response(
            AgentTraceEvent::Completed {
                text: Some("done".into()),
                usage: None,
            },
            &ctx(),
        );
        assert_eq!(out.len(), 1);
        assert!(out[0].is_terminal());
        match &out[0] {
            StreamResponse::StatusUpdate(ev) => {
                assert_eq!(
                    ev.status.state,
                    a2a_protocol_core::data::TaskState::Completed
                );
            }
            _ => panic!("expected StatusUpdate"),
        }
    }

    #[test]
    fn failed_maps_to_terminal_failed() {
        let out = map_trace_to_stream_response(
            AgentTraceEvent::Failed {
                message: "error".into(),
            },
            &ctx(),
        );
        assert_eq!(out.len(), 1);
        assert!(out[0].is_terminal());
        match &out[0] {
            StreamResponse::StatusUpdate(ev) => {
                assert_eq!(ev.status.state, a2a_protocol_core::data::TaskState::Failed);
            }
            _ => panic!("expected StatusUpdate"),
        }
    }

    #[test]
    fn unmapped_events_return_empty() {
        let unmapped = vec![
            AgentTraceEvent::ReasoningDelta { delta: "x".into() },
            AgentTraceEvent::ToolCallArgsDelta {
                index: 0,
                id: "c".into(),
                delta: "{}".into(),
            },
            AgentTraceEvent::ContextTrimmed {
                evicted_count: 1,
                remaining_count: 5,
            },
        ];
        let c = ctx();
        for event in unmapped {
            assert!(
                map_trace_to_stream_response(event, &c).is_empty(),
                "Expected empty for unmapped event"
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod broadcast_lag_tests {
    use crate::streaming::broadcast::StreamBroadcast;
    use futures::StreamExt;
    use tokio::sync::broadcast::error::RecvError;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn test_stream_broadcast_lagged_receiver_reports_lag() {
        let broadcast = StreamBroadcast::new(2);
        let mut rx = broadcast.subscribe();

        broadcast.emit("e1".to_string());
        broadcast.emit("e2".to_string());
        broadcast.emit("e3".to_string());

        match rx.recv().await {
            Err(RecvError::Lagged(skipped)) => {
                assert_eq!(skipped, 1, "expected exactly one skipped message");
            }
            other => panic!("expected lagged receiver error, got {:?}", other),
        }

        let next = rx.recv().await.expect("receive oldest retained event");
        assert_eq!(next, "e2");
    }

    #[tokio::test]
    async fn test_stream_broadcast_terminal_event_still_delivered_after_lag() {
        let broadcast = StreamBroadcast::new(4);
        let mut fast = broadcast.subscribe();
        let mut slow = broadcast.subscribe();

        let fast_task = tokio::spawn(async move {
            loop {
                let item = fast.recv().await.expect("fast receiver should keep up");
                if item == "RUN_FINISHED" {
                    return true;
                }
            }
        });

        for i in 0..8 {
            broadcast.emit(format!("step_{i}"));
            sleep(Duration::from_millis(1)).await;
        }

        sleep(Duration::from_millis(5)).await;
        let lag = slow.recv().await.expect_err("slow receiver should lag");
        assert!(matches!(lag, RecvError::Lagged(_)));

        broadcast.emit("RUN_FINISHED".to_string());

        let fast_seen_terminal = fast_task.await.expect("fast receiver task should complete");
        assert!(
            fast_seen_terminal,
            "fast subscriber should observe terminal event"
        );

        let mut slow_seen_terminal = false;
        for _ in 0..6 {
            match slow.recv().await {
                Ok(item) if item == "RUN_FINISHED" => {
                    slow_seen_terminal = true;
                    break;
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => continue,
                Err(other) => panic!("unexpected slow receiver error: {:?}", other),
            }
        }
        assert!(
            slow_seen_terminal,
            "slow subscriber should still eventually observe terminal event"
        );
    }

    #[tokio::test]
    async fn test_to_stream_yields_all_items_and_terminates_on_drop() {
        let broadcast = StreamBroadcast::<String>::new(16);
        let stream = broadcast.to_stream();

        let collector = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(2), stream.collect::<Vec<_>>())
                .await
                .expect("stream should terminate within timeout")
        });

        broadcast.emit("a".into());
        broadcast.emit("b".into());
        broadcast.emit("c".into());
        drop(broadcast);

        let items = collector.await.expect("collector task");
        assert_eq!(items, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn test_to_stream_skips_lagged_items_silently() {
        let broadcast = StreamBroadcast::<u32>::new(2);
        let stream = broadcast.to_stream();

        broadcast.emit(1);
        broadcast.emit(2);
        broadcast.emit(3);
        broadcast.emit(4);
        drop(broadcast);

        let items: Vec<u32> =
            tokio::time::timeout(Duration::from_secs(2), stream.collect::<Vec<_>>())
                .await
                .expect("stream should terminate within timeout");

        assert!(
            items.len() <= 4,
            "should not exceed emitted count, got {items:?}"
        );
        assert!(
            items.last() == Some(&4),
            "should include the last emitted item, got {items:?}"
        );
    }
}

#[cfg(test)]
mod driver_tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use futures::StreamExt;
    use serde_json::json;

    use crate::agent::trace::AgentTraceEvent;
    use crate::streaming::driver::{AgUiDriverConfig, AgUiStreamDriver, RunStatus, StreamEnricher};
    use crate::streaming::enrichers::CitationEnricher;
    use crate::streaming::{AgentIoEvent, IoEventContext};

    fn ctx() -> IoEventContext {
        IoEventContext {
            thread_id: "thread-1".into(),
            run_id: "run-1".into(),
            message_id: "msg-1".into(),
        }
    }

    #[tokio::test]
    async fn test_driver_accumulates_full_text() {
        let events = futures::stream::iter(vec![
            AgentTraceEvent::ContentDelta {
                delta: "hel".to_string(),
            },
            AgentTraceEvent::ContentDelta {
                delta: "lo".to_string(),
            },
            AgentTraceEvent::Completed {
                text: Some("hello".to_string()),
                usage: None,
            },
        ]);

        let mut stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: None,
            enrichers: Vec::new(),
        })
        .drive(events);

        while stream.next().await.is_some() {}
        let summary = stream.into_summary();
        assert_eq!(summary.full_text, "hello");
    }

    #[tokio::test]
    async fn test_driver_tracks_completed_status() {
        let events = futures::stream::iter(vec![AgentTraceEvent::Completed {
            text: Some("done".to_string()),
            usage: None,
        }]);

        let mut stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: None,
            enrichers: Vec::new(),
        })
        .drive(events);

        while stream.next().await.is_some() {}
        assert_eq!(stream.into_summary().status, RunStatus::Completed);
    }

    #[tokio::test]
    async fn test_driver_tracks_failed_status() {
        let events = futures::stream::iter(vec![AgentTraceEvent::Failed {
            message: "boom".to_string(),
        }]);

        let mut stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: None,
            enrichers: Vec::new(),
        })
        .drive(events);

        while stream.next().await.is_some() {}
        assert_eq!(stream.into_summary().status, RunStatus::Failed);
    }

    #[tokio::test]
    async fn test_driver_tracks_input_required_status() {
        let events = futures::stream::iter(vec![AgentTraceEvent::TurnCompleted {
            turn: 1,
            finish_reason: Some("input_required".to_string()),
        }]);

        let mut stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: None,
            enrichers: Vec::new(),
        })
        .drive(events);

        while stream.next().await.is_some() {}
        assert_eq!(stream.into_summary().status, RunStatus::InputRequired);
    }

    #[tokio::test]
    async fn test_driver_cancellation_emits_cancelled_event() {
        let cancel_flag = Arc::new(AtomicBool::new(true));
        let events = futures::stream::iter(vec![AgentTraceEvent::ContentDelta {
            delta: "never".to_string(),
        }]);

        let mut stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: Some(cancel_flag),
            enrichers: Vec::new(),
        })
        .drive(events);

        let mut out = Vec::new();
        while let Some(item) = stream.next().await {
            out.push(item);
        }
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], AgentIoEvent::RunStarted { .. }));
        assert!(matches!(
            &out[1],
            AgentIoEvent::Custom { name, .. } if name == "run_cancelled"
        ));
        assert_eq!(stream.into_summary().status, RunStatus::Cancelled);
    }

    struct TestEnricher;

    impl StreamEnricher for TestEnricher {
        fn enrich(&self, event: &AgentTraceEvent, _ctx: &IoEventContext) -> Vec<AgentIoEvent> {
            if matches!(event, AgentTraceEvent::ToolCallCompleted { .. }) {
                vec![AgentIoEvent::Custom {
                    name: "enriched".to_string(),
                    value: json!({"ok": true}),
                }]
            } else {
                Vec::new()
            }
        }
    }

    #[tokio::test]
    async fn test_driver_enricher_interleaves_events() {
        let events = futures::stream::iter(vec![AgentTraceEvent::ToolCallCompleted {
            index: 0,
            id: "call_1".to_string(),
            name: "search".to_string(),
            result: json!({"result": "ok"}),
            duration_ms: 10,
            success: true,
        }]);

        let mut stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: None,
            enrichers: vec![Box::new(TestEnricher)],
        })
        .drive(events);

        let mut out = Vec::new();
        while let Some(item) = stream.next().await {
            out.push(item);
        }
        let types: Vec<&str> = out.iter().map(AgentIoEvent::wire_type).collect();
        assert_eq!(types, vec!["RUN_STARTED", "TOOL_CALL_RESULT", "CUSTOM"]);
    }

    #[tokio::test]
    async fn test_with_summary_handle_events_pass_through() {
        let events = futures::stream::iter(vec![
            AgentTraceEvent::ContentDelta {
                delta: "hello".to_string(),
            },
            AgentTraceEvent::Completed {
                text: Some("hello".to_string()),
                usage: None,
            },
        ]);

        let stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: None,
            enrichers: Vec::new(),
        })
        .drive(events);

        let (tracked, _handle) = stream.with_summary_handle();
        let mut tracked = Box::pin(tracked);

        let mut out = Vec::new();
        while let Some(item) = tracked.next().await {
            out.push(item);
        }
        let types: Vec<&str> = out.iter().map(AgentIoEvent::wire_type).collect();
        assert_eq!(
            types,
            vec!["RUN_STARTED", "TEXT_MESSAGE_CONTENT", "RUN_FINISHED"]
        );
    }

    #[tokio::test]
    async fn test_with_summary_handle_captures_summary() {
        let events = futures::stream::iter(vec![
            AgentTraceEvent::ContentDelta {
                delta: "hi".to_string(),
            },
            AgentTraceEvent::ToolCallCompleted {
                index: 0,
                id: "c1".to_string(),
                name: "search".to_string(),
                result: json!({"ok": true}),
                duration_ms: 5,
                success: true,
            },
            AgentTraceEvent::Completed {
                text: Some("hi".to_string()),
                usage: Some(llm_client::Usage {
                    prompt_tokens: Some(10),
                    completion_tokens: Some(5),
                    total_tokens: Some(15),
                }),
            },
        ]);

        let stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: None,
            enrichers: Vec::new(),
        })
        .drive(events);

        let (tracked, handle) = stream.with_summary_handle();
        let mut tracked = Box::pin(tracked);

        while tracked.next().await.is_some() {}

        let summary = handle.get().await.expect("summary should be available");
        assert_eq!(summary.full_text, "hi");
        assert_eq!(summary.status, RunStatus::Completed);
        assert_eq!(summary.tool_calls_count, 1);
        assert!(summary.usage.is_some());
        assert_eq!(summary.usage.unwrap().total_tokens, Some(15));
    }

    #[tokio::test]
    async fn test_with_summary_handle_cancelled_summary() {
        let cancel_flag = Arc::new(AtomicBool::new(true));
        let events = futures::stream::iter(vec![AgentTraceEvent::ContentDelta {
            delta: "ignored".to_string(),
        }]);

        let stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: Some(cancel_flag),
            enrichers: Vec::new(),
        })
        .drive(events);

        let (tracked, handle) = stream.with_summary_handle();
        let mut tracked = Box::pin(tracked);

        while tracked.next().await.is_some() {}

        let summary = handle.get().await.expect("summary should be available");
        assert_eq!(summary.status, RunStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_driver_citation_enricher() {
        let events = futures::stream::iter(vec![AgentTraceEvent::ToolCallCompleted {
            index: 0,
            id: "call_1".to_string(),
            name: "web_search".to_string(),
            result: json!({
                "results": [
                    {
                        "url": "https://example.com",
                        "title": "Example",
                        "content": "example snippet"
                    }
                ]
            }),
            duration_ms: 10,
            success: true,
        }]);

        let mut stream = AgUiStreamDriver::new(AgUiDriverConfig {
            ctx: ctx(),
            cancel_flag: None,
            enrichers: vec![Box::new(CitationEnricher::default())],
        })
        .drive(events);

        let mut out = Vec::new();
        while let Some(item) = stream.next().await {
            out.push(item);
        }
        let citation_event = out.into_iter().find_map(|event| match event {
            AgentIoEvent::Custom { name, value } if name == "citations_updated" => Some(value),
            _ => None,
        });
        let citation = citation_event.expect("expected citations_updated custom event");
        assert_eq!(citation["messageId"], "msg-1");
        assert_eq!(citation["items"][0]["url"], "https://example.com");
    }
}
