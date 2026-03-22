use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentIoEvent {
    RunStarted {
        thread_id: String,
        run_id: String,
    },
    RunFinished {
        thread_id: String,
        run_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
    },
    RunError {
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    StepStarted {
        step_name: String,
    },
    StepFinished {
        step_name: String,
    },
    TextMessageStart {
        message_id: String,
        role: String,
    },
    TextMessageContent {
        message_id: String,
        delta: String,
    },
    TextMessageEnd {
        message_id: String,
    },
    ReasoningStart {
        message_id: String,
    },
    ReasoningMessageStart {
        message_id: String,
        role: String,
    },
    ReasoningMessageContent {
        message_id: String,
        delta: String,
    },
    ReasoningMessageEnd {
        message_id: String,
    },
    ReasoningEnd {
        message_id: String,
    },
    ToolCallStart {
        tool_call_id: String,
        tool_call_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        parent_message_id: Option<String>,
    },
    ToolCallArgs {
        tool_call_id: String,
        delta: String,
    },
    ToolCallEnd {
        tool_call_id: String,
    },
    ToolCallResult {
        tool_call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        content: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<String>,
    },
    DelegationStarted {
        delegation_id: String,
        tool_call_id: String,
        subagent: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    DelegationProgress {
        delegation_id: String,
        tool_call_id: String,
        subagent: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        status: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    DelegationFinished {
        delegation_id: String,
        tool_call_id: String,
        subagent: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        result: Option<Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    DelegationFailed {
        delegation_id: String,
        tool_call_id: String,
        subagent: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        error_kind: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    DelegationInputRequired {
        delegation_id: String,
        tool_call_id: String,
        subagent: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        task_id: Option<String>,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<Value>,
    },
    Custom {
        name: String,
        value: Value,
    },
}

impl AgentIoEvent {
    pub fn wire_type(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::RunFinished { .. } => "run_finished",
            Self::RunError { .. } => "run_error",
            Self::StepStarted { .. } => "step_started",
            Self::StepFinished { .. } => "step_finished",
            Self::TextMessageStart { .. } => "text_message_start",
            Self::TextMessageContent { .. } => "text_message_content",
            Self::TextMessageEnd { .. } => "text_message_end",
            Self::ReasoningStart { .. } => "reasoning_start",
            Self::ReasoningMessageStart { .. } => "reasoning_message_start",
            Self::ReasoningMessageContent { .. } => "reasoning_message_content",
            Self::ReasoningMessageEnd { .. } => "reasoning_message_end",
            Self::ReasoningEnd { .. } => "reasoning_end",
            Self::ToolCallStart { .. } => "tool_call_start",
            Self::ToolCallArgs { .. } => "tool_call_args",
            Self::ToolCallEnd { .. } => "tool_call_end",
            Self::ToolCallResult { .. } => "tool_call_result",
            Self::DelegationStarted { .. } => "delegation_started",
            Self::DelegationProgress { .. } => "delegation_progress",
            Self::DelegationFinished { .. } => "delegation_finished",
            Self::DelegationFailed { .. } => "delegation_failed",
            Self::DelegationInputRequired { .. } => "delegation_input_required",
            Self::Custom { .. } => "custom",
        }
    }
}

#[derive(Debug, Clone)]
pub struct IoEventContext {
    pub thread_id: String,
    pub run_id: String,
    pub message_id: String,
}
