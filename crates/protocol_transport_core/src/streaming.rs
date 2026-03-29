//! Streaming timeout policy and idle-timeout stream wrapper.
//!
//! `StreamingPolicy` is unconditional (compiled for WASM + native).
//! `IdleTimeoutStream` is native-only — requires tokio timers.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Three-clock streaming timeout configuration.
///
/// Unconditional: compiled for both WASM and native targets.
/// On WASM, values are stored for config parity but not enforced
/// until WASI 0.3 streaming support is available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingPolicy {
    /// TCP + TLS handshake timeout. Default: 10s.
    pub connect_ms: u64,
    /// Time until first data chunk arrives (after headers). Default: 45s.
    pub first_byte_ms: u64,
    /// Max silence between consecutive chunks — resets on each chunk. Default: 90s.
    pub idle_ms: u64,
}

impl Default for StreamingPolicy {
    fn default() -> Self {
        Self {
            connect_ms: 10_000,
            first_byte_ms: 45_000,
            idle_ms: 90_000,
        }
    }
}

impl StreamingPolicy {
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_millis(self.connect_ms)
    }

    pub fn first_byte_timeout(&self) -> Duration {
        Duration::from_millis(self.first_byte_ms)
    }

    pub fn idle_timeout(&self) -> Duration {
        Duration::from_millis(self.idle_ms)
    }
}

/// Per-request RPC timeout (wall-clock). Applied to non-streaming calls.
pub const RPC_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Native-only: IdleTimeoutStream
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
mod idle_timeout_impl {
    use futures::{Future, Stream};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::time::{Sleep, sleep};

    /// Stream wrapper that terminates when no item arrives within `idle_timeout`.
    ///
    /// The timer resets on every yielded item — a stream emitting one token
    /// every 500ms runs indefinitely. A stream that goes silent for
    /// `idle_timeout` is terminated with `None` (clean EOF).
    pub struct IdleTimeoutStream<S> {
        inner: Pin<Box<S>>,
        idle_timeout: Duration,
        deadline: Pin<Box<Sleep>>,
    }

    impl<S> IdleTimeoutStream<S>
    where
        S: Stream,
    {
        pub fn new(inner: S, idle_timeout: Duration) -> Self {
            Self {
                inner: Box::pin(inner),
                idle_timeout,
                deadline: Box::pin(sleep(idle_timeout)),
            }
        }
    }

    impl<S> Stream for IdleTimeoutStream<S>
    where
        S: Stream + Unpin,
    {
        type Item = S::Item;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let idle = self.idle_timeout;

            // First, try polling the inner stream.
            if let Poll::Ready(item) = Pin::new(&mut *self.inner).poll_next(cx) {
                match item {
                    Some(val) => {
                        self.deadline
                            .as_mut()
                            .reset(tokio::time::Instant::now() + idle);
                        return Poll::Ready(Some(val));
                    }
                    None => return Poll::Ready(None),
                }
            }

            // Inner stream is pending — check idle deadline.
            match self.deadline.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use idle_timeout_impl::IdleTimeoutStream;

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use futures::StreamExt;
    use futures::stream;
    use std::time::Duration;

    #[test]
    fn streaming_policy_defaults() {
        let p = StreamingPolicy::default();
        assert_eq!(p.connect_ms, 10_000);
        assert_eq!(p.first_byte_ms, 45_000);
        assert_eq!(p.idle_ms, 90_000);
        assert_eq!(p.connect_timeout(), Duration::from_secs(10));
    }

    #[tokio::test]
    async fn idle_timeout_stream_passes_items() {
        let inner = stream::iter(vec![1, 2, 3]);
        let wrapped = IdleTimeoutStream::new(inner, Duration::from_secs(10));
        let collected: Vec<_> = wrapped.collect().await;
        assert_eq!(collected, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn idle_timeout_stream_terminates_on_silence() {
        let (tx, rx) = tokio::sync::mpsc::channel::<i32>(10);
        let inner = tokio_stream::wrappers::ReceiverStream::new(rx);
        let mut wrapped = IdleTimeoutStream::new(inner, Duration::from_millis(50));

        tx.send(1).await.unwrap();
        assert_eq!(wrapped.next().await, Some(1));

        // Don't send anything — idle timeout should fire.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(wrapped.next().await, None);
    }
}
