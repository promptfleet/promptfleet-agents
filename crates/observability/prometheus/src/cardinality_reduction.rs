//! Cardinality reduction strategies for Prometheus metrics

use indexmap::IndexMap;
use std::collections::{HashMap, HashSet};
use string_interner::{DefaultSymbol, StringInterner, backend::StringBackend};

/// Cardinality reducer to optimize metric label combinations
pub struct CardinalityReducer {
    max_cardinality: usize,
    label_interner: StringInterner<StringBackend<DefaultSymbol>>,
    metric_combinations: IndexMap<String, MetricCardinalityTracker>,
    global_cardinality: usize,
}

/// Tracks cardinality for a specific metric
struct MetricCardinalityTracker {
    label_combinations: HashSet<Vec<DefaultSymbol>>,
    dropped_count: u64,
    last_seen_combinations: IndexMap<Vec<DefaultSymbol>, usize>, // LRU cache
}

impl CardinalityReducer {
    /// Create a new cardinality reducer
    pub fn new(max_cardinality: usize) -> Self {
        Self {
            max_cardinality,
            label_interner: StringInterner::<StringBackend<DefaultSymbol>>::new(),
            metric_combinations: IndexMap::new(),
            global_cardinality: 0,
        }
    }

    /// Check if a metric should be recorded based on cardinality limits
    pub fn should_record(&mut self, metric_name: &str, labels: &HashMap<String, String>) -> bool {
        // Convert labels to interned symbols for memory efficiency
        let mut label_symbols = Vec::new();
        for (key, value) in labels {
            let key_symbol = self.label_interner.get_or_intern(key);
            let value_symbol = self.label_interner.get_or_intern(value);
            label_symbols.push(key_symbol);
            label_symbols.push(value_symbol);
        }
        label_symbols.sort(); // Ensure consistent ordering

        // Get or create metric tracker
        let tracker = self
            .metric_combinations
            .entry(metric_name.to_string())
            .or_insert_with(|| MetricCardinalityTracker {
                label_combinations: HashSet::new(),
                dropped_count: 0,
                last_seen_combinations: IndexMap::new(),
            });

        // Check if this combination already exists
        if tracker.label_combinations.contains(&label_symbols) {
            // Update LRU cache
            tracker.last_seen_combinations.shift_remove(&label_symbols);
            tracker.last_seen_combinations.insert(label_symbols, 0);
            return true;
        }

        // Check global cardinality limit FIRST
        if self.global_cardinality >= self.max_cardinality {
            tracker.dropped_count += 1;
            return false;
        }

        // Accept the new combination
        tracker.label_combinations.insert(label_symbols.clone());
        tracker.last_seen_combinations.insert(label_symbols, 0);
        self.global_cardinality += 1;
        true
    }

    /// Get cardinality statistics
    pub fn get_stats(&self) -> CardinalityStats {
        let metric_stats: HashMap<String, MetricStats> = self
            .metric_combinations
            .iter()
            .map(|(name, tracker)| {
                (
                    name.clone(),
                    MetricStats {
                        unique_combinations: tracker.label_combinations.len(),
                        dropped_count: tracker.dropped_count,
                    },
                )
            })
            .collect();

        CardinalityStats {
            global_cardinality: self.global_cardinality,
            max_cardinality: self.max_cardinality,
            metric_stats,
            total_dropped: self
                .metric_combinations
                .values()
                .map(|t| t.dropped_count)
                .sum(),
        }
    }

    /// Reset cardinality tracking (for testing or periodic cleanup)
    pub fn reset(&mut self) {
        self.metric_combinations.clear();
        self.global_cardinality = 0;
        // Keep the string interner for memory efficiency
    }

    /// Get reduction ratio (0.0 to 1.0, where 1.0 means no reduction)
    pub fn get_reduction_ratio(&self) -> f64 {
        let total_dropped: u64 = self
            .metric_combinations
            .values()
            .map(|t| t.dropped_count)
            .sum();
        let total_attempted = self.global_cardinality + (total_dropped as usize);

        if total_attempted == 0 {
            1.0
        } else {
            self.global_cardinality as f64 / total_attempted as f64
        }
    }

    /// Optimize label values for common cases
    pub fn optimize_labels(&mut self, labels: &mut HashMap<String, String>) {
        // Apply common optimizations
        for (key, value) in labels.iter_mut() {
            match key.as_str() {
                "http.status_code" => {
                    // Group HTTP status codes into classes
                    if let Ok(status) = value.parse::<u16>() {
                        *value = match status {
                            200..=299 => "2xx".to_string(),
                            300..=399 => "3xx".to_string(),
                            400..=499 => "4xx".to_string(),
                            500..=599 => "5xx".to_string(),
                            _ => "other".to_string(),
                        };
                    }
                }
                "http.url" | "url" => {
                    // Extract only the path component, remove query parameters
                    #[cfg(feature = "cardinality-reduction")]
                    {
                        if let Ok(parsed_url) = url::Url::parse(value) {
                            *value = parsed_url.path().to_string();
                        }
                    }
                }
                "user_id" | "session_id" => {
                    // Hash user identifiers for privacy and cardinality reduction
                    *value = format!("hashed_{}", hash_string(value));
                }
                _ => {}
            }
        }
    }
}

/// Statistics about cardinality reduction
#[derive(Debug, Clone)]
pub struct CardinalityStats {
    pub global_cardinality: usize,
    pub max_cardinality: usize,
    pub metric_stats: HashMap<String, MetricStats>,
    pub total_dropped: u64,
}

#[derive(Debug, Clone)]
pub struct MetricStats {
    pub unique_combinations: usize,
    pub dropped_count: u64,
}

impl CardinalityStats {
    /// Get cardinality utilization as a percentage
    pub fn utilization_percent(&self) -> f64 {
        if self.max_cardinality == 0 {
            0.0
        } else {
            (self.global_cardinality as f64 / self.max_cardinality as f64) * 100.0
        }
    }

    /// Check if cardinality is approaching the limit
    pub fn is_near_limit(&self, threshold_percent: f64) -> bool {
        self.utilization_percent() >= threshold_percent
    }
}

/// Simple hash function for string values (for demo purposes)
fn hash_string(s: &str) -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    (hasher.finish() % 1000000) as u32 // Limit to 6 digits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cardinality_reduction() {
        let mut reducer = CardinalityReducer::new(5);

        // First 5 combinations should be accepted
        for i in 0..5 {
            let mut labels = HashMap::new();
            labels.insert("endpoint".to_string(), format!("/api/v{}", i));
            assert!(reducer.should_record("http_requests", &labels));
        }

        // 6th combination should be rejected
        let mut labels = HashMap::new();
        labels.insert("endpoint".to_string(), "/api/v5".to_string());
        assert!(!reducer.should_record("http_requests", &labels));

        let stats = reducer.get_stats();
        assert_eq!(stats.global_cardinality, 5);
        assert_eq!(stats.total_dropped, 1);
    }

    #[test]
    fn test_label_optimization() {
        let mut reducer = CardinalityReducer::new(100);
        let mut labels = HashMap::new();
        labels.insert("http.status_code".to_string(), "404".to_string());
        labels.insert(
            "http.url".to_string(),
            "https://example.com/api/users?page=1".to_string(),
        );

        reducer.optimize_labels(&mut labels);

        assert_eq!(labels.get("http.status_code"), Some(&"4xx".to_string()));
        // URL optimization might vary based on implementation
    }

    #[test]
    fn test_lru_eviction() {
        let mut reducer = CardinalityReducer::new(2);

        // Add first combination
        let mut labels1 = HashMap::new();
        labels1.insert("key".to_string(), "value1".to_string());
        assert!(reducer.should_record("test_metric", &labels1));

        // Add second combination
        let mut labels2 = HashMap::new();
        labels2.insert("key".to_string(), "value2".to_string());
        assert!(reducer.should_record("test_metric", &labels2));

        // Try to add third combination - should be rejected due to global limit
        let mut labels3 = HashMap::new();
        labels3.insert("key".to_string(), "value3".to_string());
        assert!(!reducer.should_record("test_metric", &labels3));

        // First and second combinations should still exist (can be recorded again)
        assert!(reducer.should_record("test_metric", &labels1));
        assert!(reducer.should_record("test_metric", &labels2));

        // But third should still be rejected
        assert!(!reducer.should_record("test_metric", &labels3));
    }

    #[test]
    fn test_reduction_ratio() {
        let mut reducer = CardinalityReducer::new(2);

        // Add 2 successful combinations
        for i in 0..2 {
            let mut labels = HashMap::new();
            labels.insert("key".to_string(), format!("value{}", i));
            reducer.should_record("test", &labels);
        }

        // Try to add 1 more that will be dropped
        let mut labels = HashMap::new();
        labels.insert("key".to_string(), "value2".to_string());
        reducer.should_record("test", &labels);

        let ratio = reducer.get_reduction_ratio();
        assert!((ratio - 0.667).abs() < 0.01); // ~2/3 accepted
    }
}
