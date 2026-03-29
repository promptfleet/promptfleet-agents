//! RAII / scoped trace-context demo (observability_core)
//!
//! The foundational “RAII guard” in `observability_core` is `with_context(...)`:
//! it sets a thread-local trace context for the duration of a closure and restores
//! the previous context even if the closure panics.

use observability_core::{TraceContext, get_current_context, with_context};

fn main() {
    println!("🔒 Scoped trace context demo");
    println!("============================\n");

    println!("Before: {:?}", get_current_context());

    let ctx = TraceContext::new_root();
    let tid = ctx.trace_id.clone();

    let out = with_context(ctx, || {
        log::info!("Processing request");
        get_current_context().unwrap().trace_id
    });

    println!("Inside returned trace_id={out}");
    println!("Expected trace_id={tid}");
    println!("After: {:?}", get_current_context());
}

#[cfg(test)]
mod tests {
    use super::*;
    use observability_core::clear_current_context;

    #[test]
    fn scoped_context_is_panic_safe() {
        clear_current_context();
        assert!(get_current_context().is_none());

        let res = std::panic::catch_unwind(|| {
            let ctx = TraceContext::new_root();
            with_context(ctx, || panic!("boom"));
        });

        assert!(res.is_err());
        assert!(get_current_context().is_none());
    }
}
