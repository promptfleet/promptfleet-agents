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
        (_base @ Value::Array(_), overlay @ Value::Array(_)) => overlay,
        (_base, overlay) => overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn overlapping_keys_overlay_wins() {
        let base = json!({"a": 1, "b": 2});
        let overlay = json!({"b": 99, "c": 3});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged, json!({"a": 1, "b": 99, "c": 3}));
    }

    #[test]
    fn base_keys_preserved_when_absent_in_overlay() {
        let base = json!({"x": "hello", "y": 42});
        let overlay = json!({"z": true});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged, json!({"x": "hello", "y": 42, "z": true}));
    }

    #[test]
    fn overlay_array_replaces_base_array() {
        let base = json!({"tags": [1, 2, 3]});
        let overlay = json!({"tags": [4, 5]});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged["tags"], json!([4, 5]));
    }

    #[test]
    fn nested_objects_merge_recursively() {
        let base = json!({"db": {"host": "localhost", "port": 5432}});
        let overlay = json!({"db": {"port": 3306, "name": "mydb"}});
        let merged = deep_merge(base, overlay);
        assert_eq!(
            merged,
            json!({"db": {"host": "localhost", "port": 3306, "name": "mydb"}})
        );
    }

    #[test]
    fn deeply_nested_three_levels() {
        let base = json!({"a": {"b": {"c": 1, "d": 2}}});
        let overlay = json!({"a": {"b": {"c": 99}}});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged, json!({"a": {"b": {"c": 99, "d": 2}}}));
    }

    #[test]
    fn non_object_overlay_replaces_base() {
        let base = json!({"key": "old"});
        let overlay = json!({"key": "new"});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged["key"], json!("new"));
    }

    #[test]
    fn overlay_null_replaces_base_value() {
        let base = json!({"a": 1, "b": 2});
        let overlay = json!({"a": null});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged, json!({"a": null, "b": 2}));
    }

    #[test]
    fn null_base_replaced_by_overlay() {
        let base = Value::Null;
        let overlay = json!({"key": "value"});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged, json!({"key": "value"}));
    }

    #[test]
    fn both_null_stays_null() {
        let merged = deep_merge(Value::Null, Value::Null);
        assert_eq!(merged, Value::Null);
    }

    #[test]
    fn scalar_overlay_replaces_object_base() {
        let base = json!({"nested": {"a": 1}});
        let overlay = json!({"nested": "flat_now"});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged["nested"], json!("flat_now"));
    }

    #[test]
    fn object_overlay_replaces_scalar_base() {
        let base = json!({"key": "string"});
        let overlay = json!({"key": {"nested": true}});
        let merged = deep_merge(base, overlay);
        assert_eq!(merged["key"], json!({"nested": true}));
    }

    #[test]
    fn empty_objects_merge_to_empty() {
        let merged = deep_merge(json!({}), json!({}));
        assert_eq!(merged, json!({}));
    }

    #[test]
    fn empty_overlay_preserves_base() {
        let base = json!({"a": 1, "b": {"c": 2}});
        let merged = deep_merge(base.clone(), json!({}));
        assert_eq!(merged, base);
    }

    #[test]
    fn empty_base_takes_overlay() {
        let overlay = json!({"x": 10});
        let merged = deep_merge(json!({}), overlay.clone());
        assert_eq!(merged, overlay);
    }
}
