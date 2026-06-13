//! Runtime helpers that are useful for HTTP services and long-running workers.

use std::time::Duration;

use crate::{LogLevel, Obs, ObsHandle, ServiceInstrumentationExt, ServiceStatus};

pub fn start_background_flush_loop(obs: Obs) -> tokio::task::JoinHandle<()> {
    let interval_ms = std::env::var("PF_OBS_FLUSH_INTERVAL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(2_000);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(err) = obs.maybe_flush() {
                let message = err.to_string();
                obs.log_service_event(
                    LogLevel::Warn,
                    "observability flush failed",
                    "observability",
                    "flush",
                    ServiceStatus::Error,
                );
                obs.log_kv(
                    LogLevel::Warn,
                    "observability flush error",
                    &[
                        ("component", "observability"),
                        ("operation", "flush"),
                        ("error", message.as_str()),
                    ],
                );
            }
        }
    })
}
