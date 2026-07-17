use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RunFinishedOutcome {
    Success,
    Interrupt { interrupts: Vec<Interrupt> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Interrupt {
    pub id: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
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
        #[serde(skip_serializing_if = "Option::is_none")]
        outcome: Option<RunFinishedOutcome>,
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
    StateSnapshot {
        snapshot: Value,
    },
    StateDelta {
        delta: Vec<Value>,
    },
    MessagesSnapshot {
        messages: Vec<Value>,
    },
    Custom {
        name: String,
        value: Value,
    },
}

impl AgentIoEvent {
    pub fn wire_type(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "RUN_STARTED",
            Self::RunFinished { .. } => "RUN_FINISHED",
            Self::RunError { .. } => "RUN_ERROR",
            Self::StepStarted { .. } => "STEP_STARTED",
            Self::StepFinished { .. } => "STEP_FINISHED",
            Self::TextMessageStart { .. } => "TEXT_MESSAGE_START",
            Self::TextMessageContent { .. } => "TEXT_MESSAGE_CONTENT",
            Self::TextMessageEnd { .. } => "TEXT_MESSAGE_END",
            Self::ReasoningStart { .. } => "REASONING_START",
            Self::ReasoningMessageStart { .. } => "REASONING_MESSAGE_START",
            Self::ReasoningMessageContent { .. } => "REASONING_MESSAGE_CONTENT",
            Self::ReasoningMessageEnd { .. } => "REASONING_MESSAGE_END",
            Self::ReasoningEnd { .. } => "REASONING_END",
            Self::ToolCallStart { .. } => "TOOL_CALL_START",
            Self::ToolCallArgs { .. } => "TOOL_CALL_ARGS",
            Self::ToolCallEnd { .. } => "TOOL_CALL_END",
            Self::ToolCallResult { .. } => "TOOL_CALL_RESULT",
            Self::StateSnapshot { .. } => "STATE_SNAPSHOT",
            Self::StateDelta { .. } => "STATE_DELTA",
            Self::MessagesSnapshot { .. } => "MESSAGES_SNAPSHOT",
            Self::Custom { .. } => "CUSTOM",
        }
    }
}

#[derive(Debug, Clone)]
pub struct IoEventContext {
    pub thread_id: String,
    pub run_id: String,
    pub message_id: String,
}
