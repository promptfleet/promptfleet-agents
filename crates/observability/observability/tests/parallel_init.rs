use observability::{Obs, ObservabilityConfig};
use std::thread;

#[test]
fn test_parallel_obs_init_is_safe() {
    let handles: Vec<_> = (0..32)
        .map(|_| thread::spawn(|| Obs::init(ObservabilityConfig::default())))
        .collect();

    for handle in handles {
        assert!(handle.join().unwrap().is_ok());
    }
}
