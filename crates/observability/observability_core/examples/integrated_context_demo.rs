//! Integrated context demo (observability_core)
//!
//! `observability_core` provides a small, WASM-safe trace context implementation
//! and RAII-style scoped execution via `with_context(...)`.
//!
//! Higher-level domain context managers (LLM/A2A/request) live in higher crates
//! like `structured_logging` and are intentionally not part of this foundation crate.

use observability_core::{get_current_context, with_context, TraceContext};

fn main() {
    println!("🔌 observability_core context demo");
    println!("==================================\n");

    println!("Before: {:?}", get_current_context());

    let root = TraceContext::new_root();
    let trace_id = root.trace_id.clone();

    let out = with_context(root, || {
        println!("Inside: {:?}", get_current_context());
        "ok"
    });

    println!("After: {:?}", get_current_context());
    println!("Result: {out} (trace_id={trace_id})");
}

#[cfg(test)]
mod tests {
    use super::*;
    use observability_core::clear_current_context;

    #[test]
    fn with_context_restores_previous() {
        clear_current_context();
        assert!(get_current_context().is_none());

        let root = TraceContext::new_root();
        let tid = root.trace_id.clone();

        let r = with_context(root, || get_current_context().unwrap().trace_id);
        assert_eq!(r, tid);

        // Context is cleared after closure (since there was no previous context).
        assert!(get_current_context().is_none());
    }
}
