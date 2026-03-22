//! Smart sampling strategies for OpenTelemetry

use observability_core::TraceContext;

/// Sampling strategies for trace data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SamplingStrategy {
    /// Always sample all traces
    AlwaysOn,
    /// Never sample any traces
    AlwaysOff,
    /// Sample at a fixed rate (0.0 to 1.0)
    TraceIdRatio(f64),
    /// Parent-based sampling
    ParentBased {
        root: Box<SamplingStrategy>,
        remote_parent_sampled: Box<SamplingStrategy>,
        remote_parent_not_sampled: Box<SamplingStrategy>,
        local_parent_sampled: Box<SamplingStrategy>,
        local_parent_not_sampled: Box<SamplingStrategy>,
    },
}

impl Default for SamplingStrategy {
    fn default() -> Self {
        // Default to 10% sampling rate
        SamplingStrategy::TraceIdRatio(0.1)
    }
}

impl SamplingStrategy {
    /// Create a trace ID ratio sampler
    pub fn trace_id_ratio(ratio: f64) -> Self {
        SamplingStrategy::TraceIdRatio(ratio.clamp(0.0, 1.0))
    }

    /// Create an always-on sampler
    pub fn always_on() -> Self {
        SamplingStrategy::AlwaysOn
    }

    /// Create an always-off sampler
    pub fn always_off() -> Self {
        SamplingStrategy::AlwaysOff
    }

    /// Create a parent-based sampler with default strategies
    pub fn parent_based() -> Self {
        SamplingStrategy::ParentBased {
            root: Box::new(SamplingStrategy::TraceIdRatio(0.1)),
            remote_parent_sampled: Box::new(SamplingStrategy::AlwaysOn),
            remote_parent_not_sampled: Box::new(SamplingStrategy::AlwaysOff),
            local_parent_sampled: Box::new(SamplingStrategy::AlwaysOn),
            local_parent_not_sampled: Box::new(SamplingStrategy::AlwaysOff),
        }
    }

    /// Determine if a trace should be sampled
    pub fn should_sample(&self, trace_context: &TraceContext) -> bool {
        match self {
            SamplingStrategy::AlwaysOn => true,
            SamplingStrategy::AlwaysOff => false,
            SamplingStrategy::TraceIdRatio(ratio) => {
                self.trace_id_ratio_decision(&trace_context.trace_id, *ratio)
            }
            SamplingStrategy::ParentBased { root, .. } => {
                // For simplicity, always use root strategy for now
                // In a full implementation, we'd check parent context
                root.should_sample(trace_context)
            }
        }
    }

    /// Make sampling decision based on trace ID ratio
    fn trace_id_ratio_decision(&self, trace_id: &str, ratio: f64) -> bool {
        if ratio <= 0.0 {
            return false;
        }
        if ratio >= 1.0 {
            return true;
        }

        // Use the last 8 characters of trace ID for sampling decision
        let suffix = if trace_id.len() >= 8 {
            &trace_id[trace_id.len() - 8..]
        } else {
            trace_id
        };

        // Convert hex to u64 and normalize to 0.0-1.0 range
        if let Ok(trace_id_int) = u64::from_str_radix(suffix, 16) {
            let normalized = trace_id_int as f64 / u64::MAX as f64;
            normalized < ratio
        } else {
            // If we can't parse trace ID, default to not sampling
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use observability_core::TraceContext;

    #[test]
    fn test_always_on_sampling() {
        let strategy = SamplingStrategy::always_on();
        let context = TraceContext::new_root();
        assert!(strategy.should_sample(&context));
    }

    #[test]
    fn test_always_off_sampling() {
        let strategy = SamplingStrategy::always_off();
        let context = TraceContext::new_root();
        assert!(!strategy.should_sample(&context));
    }

    #[test]
    fn test_trace_id_ratio_sampling() {
        let strategy = SamplingStrategy::trace_id_ratio(0.0);
        let context = TraceContext::new_root();
        assert!(!strategy.should_sample(&context));

        let strategy = SamplingStrategy::trace_id_ratio(1.0);
        assert!(strategy.should_sample(&context));
    }

    #[test]
    fn test_trace_id_ratio_consistency() {
        let strategy = SamplingStrategy::trace_id_ratio(0.5);
        let mut context = TraceContext::new_root();
        context.trace_id = "12345678901234567890123456789012".to_string();

        // Same trace ID should always give same result
        let result1 = strategy.should_sample(&context);
        let result2 = strategy.should_sample(&context);
        assert_eq!(result1, result2);
    }
}
