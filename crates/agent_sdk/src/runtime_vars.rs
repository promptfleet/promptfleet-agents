use log::{debug, warn};

/// Activation budget configuration loaded from env vars / Spin variables.
///
/// Controls SDK-side retry behavior when calling potentially cold (scaled-to-zero) agents.
/// These values override `ActivationConfig` defaults in `a2a_http_client::activation`.
///
/// Variables (all optional, defaults applied if absent):
/// - `PF_ACTIVATION_MAX_COLD_START_MS` / `pf_activation_max_cold_start_ms` (u64, default: 60000)
/// - `PF_ACTIVATION_INITIAL_BACKOFF_MS` / `pf_activation_initial_backoff_ms` (u64, default: 100)
/// - `PF_ACTIVATION_MAX_BACKOFF_MS` / `pf_activation_max_backoff_ms` (u64, default: 2000)
/// - `PF_ACTIVATION_MAX_RETRIES` / `pf_activation_max_retries` (u32, default: 3)
/// - `PF_ACTIVATION_JITTER` / `pf_activation_jitter` (bool, default: true)
#[derive(Debug, Clone)]
pub struct ActivationEnv {
    pub max_cold_start_ms: u64,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
    pub max_retries: u32,
    pub jitter: bool,
}

impl Default for ActivationEnv {
    fn default() -> Self {
        Self {
            max_cold_start_ms: 60_000,
            initial_backoff_ms: 100,
            max_backoff_ms: 2000,
            max_retries: 3,
            jitter: true,
        }
    }
}

impl ActivationEnv {
    /// Load activation configuration from env vars / Spin variables.
    /// All fields are optional -- defaults are applied when missing or unparseable.
    pub fn load_optional() -> Self {
        let mut env = Self::default();

        if let Some(raw) = get_var(
            "PF_ACTIVATION_MAX_COLD_START_MS",
            "pf_activation_max_cold_start_ms",
        ) {
            if let Ok(v) = raw.trim().parse::<u64>() {
                env.max_cold_start_ms = v;
            } else {
                warn!(
                    "Invalid PF_ACTIVATION_MAX_COLD_START_MS='{}', using default",
                    raw
                );
            }
        }

        if let Some(raw) = get_var(
            "PF_ACTIVATION_INITIAL_BACKOFF_MS",
            "pf_activation_initial_backoff_ms",
        ) {
            if let Ok(v) = raw.trim().parse::<u64>() {
                env.initial_backoff_ms = v;
            } else {
                warn!(
                    "Invalid PF_ACTIVATION_INITIAL_BACKOFF_MS='{}', using default",
                    raw
                );
            }
        }

        if let Some(raw) = get_var(
            "PF_ACTIVATION_MAX_BACKOFF_MS",
            "pf_activation_max_backoff_ms",
        ) {
            if let Ok(v) = raw.trim().parse::<u64>() {
                env.max_backoff_ms = v;
            } else {
                warn!(
                    "Invalid PF_ACTIVATION_MAX_BACKOFF_MS='{}', using default",
                    raw
                );
            }
        }

        if let Some(raw) = get_var("PF_ACTIVATION_MAX_RETRIES", "pf_activation_max_retries") {
            if let Ok(v) = raw.trim().parse::<u32>() {
                env.max_retries = v;
            } else {
                warn!("Invalid PF_ACTIVATION_MAX_RETRIES='{}', using default", raw);
            }
        }

        if let Some(raw) = get_var("PF_ACTIVATION_JITTER", "pf_activation_jitter") {
            let v = raw.trim().to_ascii_lowercase();
            env.jitter = !matches!(v.as_str(), "0" | "false" | "no" | "n" | "off");
        }

        debug!(
            "Loaded activation vars: max_cold_start_ms={} initial_backoff_ms={} max_backoff_ms={} max_retries={} jitter={}",
            env.max_cold_start_ms,
            env.initial_backoff_ms,
            env.max_backoff_ms,
            env.max_retries,
            env.jitter
        );
        env
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointMode {
    /// Only internal emit/state recording (no task_patch / respond controls)
    StateOnly,
    /// Allow updating A2A Task state/metadata for polling
    TaskObservable,
    /// Allow choosing response kind (Task vs Message)
    ResponseControl,
}

impl Default for CheckpointMode {
    fn default() -> Self {
        CheckpointMode::TaskObservable
    }
}

#[derive(Debug, Clone)]
pub struct CheckpointEnv {
    pub mode: CheckpointMode,
    /// If true, allow respond.kind="message" (only meaningful in ResponseControl mode)
    pub allow_message_response: bool,
    /// If true, mirror `internal_state` into Task metadata under `internal_state` key (when present)
    pub mirror_internal_state_to_task_meta: bool,
}

impl Default for CheckpointEnv {
    fn default() -> Self {
        Self {
            mode: CheckpointMode::TaskObservable,
            allow_message_response: false,
            mirror_internal_state_to_task_meta: true,
        }
    }
}

impl CheckpointEnv {
    /// Optional:
    /// - pf_checkpoint_mode | PF_CHECKPOINT_MODE (state_only|task_observable|response_control)
    /// - pf_checkpoint_allow_message_response | PF_CHECKPOINT_ALLOW_MESSAGE_RESPONSE (bool)
    /// - pf_checkpoint_mirror_internal_state_to_task_meta | PF_CHECKPOINT_MIRROR_INTERNAL_STATE_TO_TASK_META (bool)
    pub fn load_optional() -> Self {
        let mut env = Self::default();

        if let Some(raw) = get_var("PF_CHECKPOINT_MODE", "pf_checkpoint_mode") {
            let v = raw.trim().to_ascii_lowercase();
            env.mode = match v.as_str() {
                "state_only" => CheckpointMode::StateOnly,
                "task_observable" | "" => CheckpointMode::TaskObservable,
                "response_control" => CheckpointMode::ResponseControl,
                other => {
                    warn!(
                        "Unknown PF_CHECKPOINT_MODE='{}', defaulting to task_observable",
                        other
                    );
                    CheckpointMode::TaskObservable
                }
            };
        }

        if let Some(raw) = get_var(
            "PF_CHECKPOINT_ALLOW_MESSAGE_RESPONSE",
            "pf_checkpoint_allow_message_response",
        ) {
            let v = raw.trim().to_ascii_lowercase();
            env.allow_message_response = matches!(v.as_str(), "1" | "true" | "yes" | "y" | "on");
        }

        if let Some(raw) = get_var(
            "PF_CHECKPOINT_MIRROR_INTERNAL_STATE_TO_TASK_META",
            "pf_checkpoint_mirror_internal_state_to_task_meta",
        ) {
            let v = raw.trim().to_ascii_lowercase();
            env.mirror_internal_state_to_task_meta =
                matches!(v.as_str(), "1" | "true" | "yes" | "y" | "on");
        }

        debug!(
            "Loaded checkpoint vars: mode={:?} allow_message_response={} mirror_internal_state_to_task_meta={}",
            env.mode, env.allow_message_response, env.mirror_internal_state_to_task_meta
        );
        env
    }
}

fn get_var(upper: &str, lower: &str) -> Option<String> {
    // wasm + server: try Spin variables first, then env
    get_spin_var(lower).or_else(|| std_env_var(upper))
}

#[inline]
fn std_env_var(upper: &str) -> Option<String> {
    std::env::var(upper).ok()
}

#[cfg(all(target_arch = "wasm32", feature = "a2a-server"))]
#[inline]
fn get_spin_var(lower: &str) -> Option<String> {
    match spin_sdk::variables::get(lower) {
        Ok(v) => Some(v),
        Err(_) => None,
    }
}

#[cfg(not(all(target_arch = "wasm32", feature = "a2a-server")))]
#[inline]
fn get_spin_var(_lower: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_MUTEX
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env test lock")
    }

    #[test]
    fn activation_env_defaults() {
        let _g = env_lock();
        unsafe {
            for k in [
                "PF_ACTIVATION_MAX_COLD_START_MS",
                "PF_ACTIVATION_INITIAL_BACKOFF_MS",
                "PF_ACTIVATION_MAX_BACKOFF_MS",
                "PF_ACTIVATION_MAX_RETRIES",
                "PF_ACTIVATION_JITTER",
            ] {
                std::env::remove_var(k);
            }
        }
        let e = ActivationEnv::load_optional();
        assert_eq!(e.max_cold_start_ms, 60_000);
        assert_eq!(e.max_retries, 3);
        assert!(e.jitter);
    }

    #[test]
    fn activation_env_overrides() {
        let _g = env_lock();
        unsafe {
            std::env::set_var("PF_ACTIVATION_MAX_RETRIES", "7");
            std::env::set_var("PF_ACTIVATION_JITTER", "false");
        }
        let e = ActivationEnv::load_optional();
        assert_eq!(e.max_retries, 7);
        assert!(!e.jitter);
        unsafe {
            std::env::remove_var("PF_ACTIVATION_MAX_RETRIES");
            std::env::remove_var("PF_ACTIVATION_JITTER");
        }
    }

    #[test]
    fn activation_env_invalid_falls_back() {
        let _g = env_lock();
        unsafe {
            std::env::set_var("PF_ACTIVATION_MAX_COLD_START_MS", "not-a-number");
        }
        let e = ActivationEnv::load_optional();
        assert_eq!(e.max_cold_start_ms, 60_000);
        unsafe {
            std::env::remove_var("PF_ACTIVATION_MAX_COLD_START_MS");
        }
    }

    #[test]
    fn checkpoint_env_defaults() {
        let _g = env_lock();
        unsafe {
            std::env::remove_var("PF_CHECKPOINT_MODE");
            std::env::remove_var("PF_CHECKPOINT_ALLOW_MESSAGE_RESPONSE");
        }
        let e = CheckpointEnv::load_optional();
        assert_eq!(e.mode, CheckpointMode::TaskObservable);
        assert!(!e.allow_message_response);
    }

    #[test]
    fn checkpoint_mode_parsing() {
        let _g = env_lock();
        unsafe {
            std::env::set_var("PF_CHECKPOINT_MODE", "state_only");
        }
        assert_eq!(
            CheckpointEnv::load_optional().mode,
            CheckpointMode::StateOnly
        );
        unsafe {
            std::env::set_var("PF_CHECKPOINT_MODE", "response_control");
        }
        assert_eq!(
            CheckpointEnv::load_optional().mode,
            CheckpointMode::ResponseControl
        );
        unsafe {
            std::env::set_var("PF_CHECKPOINT_MODE", "unknown_mode");
        }
        assert_eq!(
            CheckpointEnv::load_optional().mode,
            CheckpointMode::TaskObservable
        );
        unsafe {
            std::env::remove_var("PF_CHECKPOINT_MODE");
        }
    }

    #[test]
    fn checkpoint_allow_message_response_bool() {
        let _g = env_lock();
        unsafe {
            std::env::set_var("PF_CHECKPOINT_ALLOW_MESSAGE_RESPONSE", "true");
        }
        assert!(CheckpointEnv::load_optional().allow_message_response);
        unsafe {
            std::env::set_var("PF_CHECKPOINT_ALLOW_MESSAGE_RESPONSE", "0");
        }
        assert!(!CheckpointEnv::load_optional().allow_message_response);
        unsafe {
            std::env::remove_var("PF_CHECKPOINT_ALLOW_MESSAGE_RESPONSE");
        }
    }
}
