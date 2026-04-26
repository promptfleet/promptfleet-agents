//! SDK-owned observability runtime helpers.
//!
//! This module centralizes flush semantics so telemetry is reliably exported from:
//! - native agents (periodic background flush)
//! - WASM agents (request-driven time-gated flush)

#[cfg(feature = "agent-observability")]
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(feature = "agent-observability")]
use std::time::{Duration, Instant};

#[cfg(feature = "agent-observability")]
use observability::Obs;

#[cfg(feature = "agent-observability")]
use observability::ObsHandle;

#[cfg(feature = "agent-observability")]
static GLOBAL_OBS: OnceLock<Obs> = OnceLock::new();

/// SDK-owned helper that orchestrates best-effort flushing.
///
/// Important: this does **not** change tracing/metrics APIs; components should continue
/// using the shared `Obs` handle for spans/metrics/logs.
#[cfg(feature = "agent-observability")]
#[derive(Clone)]
pub struct ObservabilityRuntime {
    obs: Obs,
    flush_interval: Duration,
    last_flush: Arc<Mutex<Option<Instant>>>,
}

#[cfg(feature = "agent-observability")]
impl ObservabilityRuntime {
    pub fn new(obs: Obs) -> Self {
        Self {
            obs,
            flush_interval: Self::flush_interval_from_env(),
            last_flush: Arc::new(Mutex::new(None)),
        }
    }

    pub fn obs(&self) -> &Obs {
        &self.obs
    }

    /// WASM-safe, request-driven flush hook.
    ///
    /// Best-effort: if a backend doesn't support flushing, this is a no-op.
    pub fn maybe_flush(&self) {
        let now = Instant::now();
        let should_flush = {
            let mut last = self.last_flush.lock().unwrap();
            match *last {
                None => {
                    *last = Some(now);
                    true
                }
                Some(prev) if now.duration_since(prev) >= self.flush_interval => {
                    *last = Some(now);
                    true
                }
                _ => false,
            }
        };

        if should_flush {
            let _ = self.obs.flush();
        }
    }

    /// Native-only periodic flush loop.
    ///
    /// This is intentionally **best-effort**:
    /// - if no Tokio runtime is available, it silently does nothing
    /// - flush errors must never crash the agent
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_background_flush_loop(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };

        let obs = self.obs.clone();
        let interval = self.flush_interval;

        handle.spawn(async move {
            loop {
                tokio::time::sleep(interval).await;

                // Prefer awaited async flush when OTEL is enabled; otherwise fall back to sync flush.
                #[cfg(feature = "agent-observability")]
                if let Some(otel) = obs.otel_plugin() {
                    let _ = otel.clone().flush_async().await;
                } else {
                    let _ = obs.flush();
                }
            }
        });
    }

    /// WASM: do not rely on background tasks for correctness.
    #[cfg(target_arch = "wasm32")]
    pub fn start_background_flush_loop(&self) {
        // No-op on WASM by design.
    }

    fn flush_interval_from_env() -> Duration {
        // Keep the env var name stable and product-specific.
        // This is separate from OTEL standard env vars.
        const KEY: &str = "PF_OBS_FLUSH_INTERVAL_MS";

        // Keep aligned with `observability_core::DEFAULT_FLUSH_INTERVAL_SECS` (currently 5s)
        // without taking a direct dependency on `observability_core` from the SDK crate.
        let default_ms: u64 = 5_000;

        std::env::var(KEY)
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(default_ms))
    }
}

#[cfg(feature = "agent-observability")]
pub fn install_global_observability(obs: Obs) -> bool {
    GLOBAL_OBS.set(obs).is_ok()
}

#[cfg(feature = "agent-observability")]
pub fn shared_observability() -> Option<Obs> {
    GLOBAL_OBS.get().cloned()
}

#[cfg(all(test, feature = "agent-observability"))]
mod tests {
    use super::ObservabilityRuntime;
    use observability::Obs;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_MUTEX
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env test lock")
    }

    #[test]
    fn maybe_flush_first_and_throttled() {
        let _g = env_lock();
        unsafe {
            std::env::remove_var("PF_OBS_FLUSH_INTERVAL_MS");
        }
        let rt = ObservabilityRuntime::new(Obs::noop());
        rt.maybe_flush();
        rt.maybe_flush();
    }

    #[test]
    fn flush_interval_from_env_custom() {
        let _g = env_lock();
        unsafe {
            std::env::set_var("PF_OBS_FLUSH_INTERVAL_MS", "1");
        }
        let rt = ObservabilityRuntime::new(Obs::noop());
        std::thread::sleep(Duration::from_millis(5));
        rt.maybe_flush();
        rt.maybe_flush();
        unsafe {
            std::env::remove_var("PF_OBS_FLUSH_INTERVAL_MS");
        }
    }
}
