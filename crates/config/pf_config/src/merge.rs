use serde_json::Value;

pub fn deep_merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut a), Value::Object(b)) => {
            for (k, v_b) in b {
                let v_a = a.remove(&k);
                a.insert(
                    k,
                    match v_a {
                        Some(v_a) => deep_merge(v_a, v_b),
                        None => v_b,
                    },
                );
            }
            Value::Object(a)
        }
        // Arrays: replace by default (documented behavior)
        (_a @ Value::Array(_), b @ Value::Array(_)) => b,
        // Otherwise: overlay wins
        (_a, b) => b,
    }
}
