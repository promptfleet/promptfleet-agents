use std::sync::Arc;
use std::time::Duration;

use agent_sdk::agent::skill::{SkillDefinition, SkillRegistry, build_wired_read_skill_tool};
use agent_sdk::agent::tools::{ToolExecutor, ToolKind, ToolRegistry, ToolSpec};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use pf_test_harness::perf::{
    collect_a2a_sse, collect_agent_io_sse, default_a2a_context, default_io_context,
    default_skill_execution_context, default_tool_context, execute_engine_scenario, execute_skill,
    execute_skill_with_context, execute_tool, execute_tool_with_context, map_a2a_sse_events,
    map_agent_io_events,
};
use pf_test_harness::pipeline::{InvokerMode, MapperKind, TestPipeline};
use pf_test_harness::scenario::LlmScenario;
use serde_json::json;
use tokio::runtime::{Builder, Runtime};

fn runtime() -> Runtime {
    Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("create benchmark runtime")
}

fn make_tool_registry() -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(ToolSpec {
        name: "echo".to_string(),
        description: Some("Echo test tool".to_string()),
        parameters: json!({"type":"object"}),
        kind: ToolKind::Function,
        strict: false,
        parallel_ok: false,
        executor: ToolExecutor::Simple(Arc::new(|args| {
            Box::pin(async move { Ok(json!({"echoed": args})) })
        })),
    });
    tools
}

fn make_context_tool_registry() -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(ToolSpec {
        name: "ctx_echo".to_string(),
        description: Some("Context-aware echo tool".to_string()),
        parameters: json!({"type":"object"}),
        kind: ToolKind::Function,
        strict: false,
        parallel_ok: false,
        executor: ToolExecutor::WithContext(Arc::new(|args, ctx| {
            Box::pin(async move {
                ctx.emit(agent_sdk::agent::trace::AgentTraceEvent::ProgressUpdate {
                    message: "ctx tool executed".to_string(),
                    progress_pct: None,
                    metadata: None,
                });
                Ok(json!({"echoed": args}))
            })
        })),
    });
    tools
}

fn base_skill_definition(id: &str, llm_callable: bool) -> SkillDefinition {
    SkillDefinition {
        id: id.to_string(),
        name: id.to_string(),
        description: format!("Benchmark skill '{id}'"),
        input_modes: vec!["application/json".to_string()],
        output_modes: vec!["application/json".to_string()],
        schema: Some(json!({"type":"object"})),
        examples: None,
        tags: None,
        instructions: Some("Benchmark skill instructions".to_string()),
        expose: true,
        llm_callable,
    }
}

fn make_skill_registry() -> SkillRegistry {
    let mut skills = SkillRegistry::new();
    skills
        .skill("lookup", |params| async move {
            Ok(json!({
                "payload": params,
                "summary": "lookup complete",
            }))
        })
        .llm_callable(true)
        .instructions("Use this skill for local lookup benchmarks.")
        .register()
        .expect("register lookup skill");
    skills
        .register_contextual_skill_for_test(
            "lookup_ctx",
            |params, exec_ctx| async move {
                Ok(json!({
                    "payload": params,
                    "has_task": exec_ctx.task_ctx.is_some(),
                    "message_type": format!("{:?}", exec_ctx.message_ctx.message_type),
                }))
            },
            base_skill_definition("lookup_ctx", false),
        )
        .expect("register context skill");
    skills
}

fn read_skill_tools() -> ToolRegistry {
    let skills = make_skill_registry();
    let tool = build_wired_read_skill_tool(Arc::new(skills)).expect("read_skill tool");
    let mut tools = ToolRegistry::new();
    tools.register(tool);
    tools
}

fn small_text_scenario() -> LlmScenario {
    LlmScenario::single_text("done")
}

fn stress_text_scenario(delta_count: usize) -> LlmScenario {
    LlmScenario::new().turn(|mut turn| {
        for idx in 0..delta_count {
            turn = turn.content(format!("d{idx}"));
        }
        turn.done("stop")
    })
}

fn typical_tool_scenario() -> LlmScenario {
    LlmScenario::tool_call_then_text("echo", json!({"q": "benchmark"}), "done")
}

fn stress_tool_scenario(
    call_count: usize,
    fragment_count: usize,
    delta_count: usize,
) -> LlmScenario {
    LlmScenario::new()
        .turn(|mut turn| {
            for idx in 0..call_count {
                turn = turn.tool_call_fragmented(
                    "echo",
                    json!({
                        "call": idx,
                        "payload": format!("payload-{idx}"),
                    }),
                    fragment_count,
                );
            }
            turn.done("tool_calls")
        })
        .turn(|mut turn| {
            for idx in 0..delta_count {
                turn = turn.content(format!("tool-result-{idx}"));
            }
            turn.done("stop")
        })
}

fn typical_read_skill_scenario() -> LlmScenario {
    LlmScenario::tool_call_then_text("read_skill", json!({"skill_id": "lookup"}), "done")
}

fn stress_read_skill_scenario(call_count: usize, delta_count: usize) -> LlmScenario {
    LlmScenario::new()
        .turn(|mut turn| {
            for _ in 0..call_count {
                turn = turn.tool_call("read_skill", json!({"skill_id": "lookup"}));
            }
            turn.done("tool_calls")
        })
        .turn(|mut turn| {
            for idx in 0..delta_count {
                turn = turn.content(format!("skill-result-{idx}"));
            }
            turn.done("stop")
        })
}

fn bench_engine_only(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("engine_only_text");

    group.bench_function("request_response_small", |b| {
        b.to_async(&rt).iter(|| async {
            execute_engine_scenario(
                small_text_scenario(),
                ToolRegistry::new(),
                InvokerMode::RequestResponse,
            )
            .await
            .expect("request-response engine");
        });
    });

    group.bench_function("streaming_small", |b| {
        b.to_async(&rt).iter(|| async {
            execute_engine_scenario(
                small_text_scenario(),
                ToolRegistry::new(),
                InvokerMode::Streaming,
            )
            .await
            .expect("streaming engine");
        });
    });

    group.bench_function("streaming_stress_deltas_64", |b| {
        b.to_async(&rt).iter(|| async {
            execute_engine_scenario(
                stress_text_scenario(64),
                ToolRegistry::new(),
                InvokerMode::Streaming,
            )
            .await
            .expect("stress engine");
        });
    });

    group.finish();
}

fn bench_engine_tool_then_text(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("engine_tool_then_text");

    group.bench_function("typical", |b| {
        b.to_async(&rt).iter(|| async {
            execute_engine_scenario(
                typical_tool_scenario(),
                make_tool_registry(),
                InvokerMode::Streaming,
            )
            .await
            .expect("tool scenario");
        });
    });

    group.bench_function("stress_calls_4_fragments_4", |b| {
        b.to_async(&rt).iter(|| async {
            execute_engine_scenario(
                stress_tool_scenario(4, 4, 32),
                make_tool_registry(),
                InvokerMode::Streaming,
            )
            .await
            .expect("stress tool scenario");
        });
    });

    group.finish();
}

fn bench_tool_only(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("tool_only");
    group.throughput(Throughput::Elements(1));

    let args = json!({"payload": "benchmark"});
    let simple_tools = make_tool_registry();
    group.bench_function("simple", |b| {
        b.to_async(&rt).iter(|| async {
            execute_tool(&simple_tools, "echo", args.clone())
                .await
                .expect("simple tool");
        });
    });

    let ctx_tools = make_context_tool_registry();
    group.bench_function("with_context", |b| {
        b.to_async(&rt).iter(|| async {
            execute_tool_with_context(
                &ctx_tools,
                "ctx_echo",
                args.clone(),
                Some(default_tool_context()),
            )
            .await
            .expect("context tool");
        });
    });

    group.finish();
}

fn bench_skill_direct(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("skill_direct");
    group.throughput(Throughput::Elements(1));

    let skills = make_skill_registry();
    let args = json!({"payload": "benchmark"});

    group.bench_function("simple", |b| {
        b.to_async(&rt).iter(|| async {
            execute_skill(&skills, "lookup", args.clone())
                .await
                .expect("simple skill");
        });
    });

    let exec_ctx = default_skill_execution_context();
    group.bench_function("with_context", |b| {
        b.to_async(&rt).iter(|| async {
            execute_skill_with_context(&skills, "lookup_ctx", args.clone(), &exec_ctx)
                .await
                .expect("context skill");
        });
    });

    group.finish();
}

fn bench_skill_read_tool(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("skill_read_tool");

    group.bench_function("typical", |b| {
        b.to_async(&rt).iter(|| async {
            execute_engine_scenario(
                typical_read_skill_scenario(),
                read_skill_tools(),
                InvokerMode::Streaming,
            )
            .await
            .expect("read skill scenario");
        });
    });

    group.bench_function("stress_calls_4", |b| {
        b.to_async(&rt).iter(|| async {
            execute_engine_scenario(
                stress_read_skill_scenario(4, 32),
                read_skill_tools(),
                InvokerMode::Streaming,
            )
            .await
            .expect("stress read skill scenario");
        });
    });

    group.finish();
}

fn bench_mapping_and_sse(c: &mut Criterion) {
    let rt = runtime();
    let trace_capture = rt
        .block_on(execute_engine_scenario(
            stress_tool_scenario(4, 4, 32),
            make_tool_registry(),
            InvokerMode::Streaming,
        ))
        .expect("precompute trace capture");
    let trace_events = trace_capture.trace_events().to_vec();
    let io_ctx = default_io_context();
    let a2a_ctx = default_a2a_context();

    let mut agui_group = c.benchmark_group("mapping_only_agui");
    agui_group.throughput(Throughput::Elements(trace_events.len() as u64));
    agui_group.bench_function(
        BenchmarkId::new("stress_trace_events", trace_events.len()),
        |b| {
            b.iter(|| {
                map_agent_io_events(&trace_events, &io_ctx);
            });
        },
    );
    agui_group.finish();

    let mut a2a_group = c.benchmark_group("mapping_only_a2a");
    a2a_group.throughput(Throughput::Elements(trace_events.len() as u64));
    a2a_group.bench_function(
        BenchmarkId::new("stress_trace_events", trace_events.len()),
        |b| {
            b.iter(|| {
                map_a2a_sse_events(&trace_events, &a2a_ctx);
            });
        },
    );
    a2a_group.finish();

    let mapped_agui = map_agent_io_events(&trace_events, &io_ctx);
    let mapped_a2a = map_a2a_sse_events(&trace_events, &a2a_ctx);

    let mut sse_group = c.benchmark_group("sse_only");
    sse_group.bench_function(BenchmarkId::new("agui_frames", mapped_agui.len()), |b| {
        b.to_async(&rt).iter(|| async {
            collect_agent_io_sse(mapped_agui.clone(), Duration::from_secs(1))
                .await
                .expect("AG-UI SSE capture");
        });
    });
    sse_group.bench_function(BenchmarkId::new("a2a_frames", mapped_a2a.len()), |b| {
        b.to_async(&rt).iter(|| async {
            collect_a2a_sse(mapped_a2a.clone(), Duration::from_secs(1))
                .await
                .expect("A2A SSE capture");
        });
    });
    sse_group.finish();
}

fn bench_end_to_end_pipeline(c: &mut Criterion) {
    let rt = runtime();
    let mut group = c.benchmark_group("end_to_end_pipeline");

    group.bench_function("agent_io_typical", |b| {
        b.to_async(&rt).iter(|| async {
            TestPipeline::new()
                .with_scenario(typical_tool_scenario())
                .with_tools(make_tool_registry())
                .with_mapper(MapperKind::AgentIo)
                .with_invoker_mode(InvokerMode::Streaming)
                .build()
                .expect("build typical pipeline")
                .run()
                .await
                .expect("run typical pipeline");
        });
    });

    group.bench_function("a2a_stress", |b| {
        b.to_async(&rt).iter(|| async {
            TestPipeline::new()
                .with_scenario(stress_tool_scenario(4, 4, 32))
                .with_tools(make_tool_registry())
                .with_mapper(MapperKind::A2aSse)
                .with_invoker_mode(InvokerMode::Streaming)
                .build()
                .expect("build stress pipeline")
                .run()
                .await
                .expect("run stress pipeline");
        });
    });

    group.finish();
}

fn configure_criterion() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
}

criterion_group!(
    name = benches;
    config = configure_criterion();
    targets =
        bench_engine_only,
        bench_engine_tool_then_text,
        bench_tool_only,
        bench_skill_direct,
        bench_skill_read_tool,
        bench_mapping_and_sse,
        bench_end_to_end_pipeline
);
criterion_main!(benches);
