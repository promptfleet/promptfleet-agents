//! Universal serialization traits for all protocols

use crate::TransportError;
use crate::error::TransportResult;
use serde::{Deserialize, Serialize};

/// **Serialization Format**
#[derive(Debug, Clone)]
pub enum SerializationFormat {
    Json,
    MessagePack,
    Protobuf,
}

/// **Universal Serializer**
pub struct Serializer {
    format: SerializationFormat,
}

impl Serializer {
    /// Create JSON serializer (most common)
    pub fn json() -> Self {
        Self {
            format: SerializationFormat::Json,
        }
    }

    /// Serialize to bytes
    pub fn serialize<T: Serialize>(&self, value: &T) -> TransportResult<Vec<u8>> {
        match self.format {
            SerializationFormat::Json => {
                serde_json::to_vec(value).map_err(|e| TransportError::Serialization(e.to_string()))
            }
            _ => Err(TransportError::Configuration(
                "Unsupported serialization format".to_string(),
            )),
        }
    }

    /// Deserialize from bytes
    pub fn deserialize<T: for<'a> Deserialize<'a>>(&self, data: &[u8]) -> TransportResult<T> {
        match self.format {
            SerializationFormat::Json => serde_json::from_slice(data)
                .map_err(|e| TransportError::Serialization(e.to_string())),
            _ => Err(TransportError::Configuration(
                "Unsupported serialization format".to_string(),
            )),
        }
    }
}

impl Default for Serializer {
    fn default() -> Self {
        Self::json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestStruct {
        id: u32,
        name: String,
        optional_field: Option<String>,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct SimpleStruct {
        value: i32,
    }

    #[test]
    fn test_serialization_format_debug() {
        let json_format = SerializationFormat::Json;
        let debug_str = format!("{:?}", json_format);
        assert!(debug_str.contains("Json"));

        let msgpack_format = SerializationFormat::MessagePack;
        let debug_str = format!("{:?}", msgpack_format);
        assert!(debug_str.contains("MessagePack"));

        let protobuf_format = SerializationFormat::Protobuf;
        let debug_str = format!("{:?}", protobuf_format);
        assert!(debug_str.contains("Protobuf"));
    }

    #[test]
    fn test_serialization_format_clone() {
        let original = SerializationFormat::Json;
        let cloned = original.clone();

        // Check they both serialize to same debug representation
        assert_eq!(format!("{:?}", original), format!("{:?}", cloned));
    }

    #[test]
    fn test_serializer_json_creation() {
        let serializer = Serializer::json();
        match serializer.format {
            SerializationFormat::Json => (),
            _ => panic!("Expected JSON format"),
        }
    }

    #[test]
    fn test_serializer_default() {
        let serializer = Serializer::default();
        match serializer.format {
            SerializationFormat::Json => (),
            _ => panic!("Default should be JSON format"),
        }
    }

    #[test]
    fn test_json_serialize_simple_struct() {
        let serializer = Serializer::json();
        let test_data = SimpleStruct { value: 42 };

        let result = serializer.serialize(&test_data);
        assert!(result.is_ok());

        let bytes = result.unwrap();
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(json_str.contains("42"));
        assert!(json_str.contains("value"));
    }

    #[test]
    fn test_json_serialize_complex_struct() {
        let serializer = Serializer::json();
        let test_data = TestStruct {
            id: 123,
            name: "test_name".to_string(),
            optional_field: Some("optional_value".to_string()),
        };

        let result = serializer.serialize(&test_data);
        assert!(result.is_ok());

        let bytes = result.unwrap();
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(json_str.contains("123"));
        assert!(json_str.contains("test_name"));
        assert!(json_str.contains("optional_value"));
    }

    #[test]
    fn test_json_serialize_with_none_field() {
        let serializer = Serializer::json();
        let test_data = TestStruct {
            id: 456,
            name: "test_with_none".to_string(),
            optional_field: None,
        };

        let result = serializer.serialize(&test_data);
        assert!(result.is_ok());

        let bytes = result.unwrap();
        let json_str = String::from_utf8(bytes).unwrap();
        assert!(json_str.contains("456"));
        assert!(json_str.contains("test_with_none"));
        // Should contain null for the None field
        assert!(json_str.contains("null"));
    }

    #[test]
    fn test_json_deserialize_simple_struct() {
        let serializer = Serializer::json();
        let json_data = br#"{"value":789}"#;

        let result: TransportResult<SimpleStruct> = serializer.deserialize(json_data);
        assert!(result.is_ok());

        let deserialized = result.unwrap();
        assert_eq!(deserialized.value, 789);
    }

    #[test]
    fn test_json_deserialize_complex_struct() {
        let serializer = Serializer::json();
        let json_data =
            br#"{"id":999,"name":"deserialized_name","optional_field":"deserialized_optional"}"#;

        let result: TransportResult<TestStruct> = serializer.deserialize(json_data);
        assert!(result.is_ok());

        let deserialized = result.unwrap();
        assert_eq!(deserialized.id, 999);
        assert_eq!(deserialized.name, "deserialized_name");
        assert_eq!(
            deserialized.optional_field,
            Some("deserialized_optional".to_string())
        );
    }

    #[test]
    fn test_json_deserialize_with_null_field() {
        let serializer = Serializer::json();
        let json_data = br#"{"id":111,"name":"null_test","optional_field":null}"#;

        let result: TransportResult<TestStruct> = serializer.deserialize(json_data);
        assert!(result.is_ok());

        let deserialized = result.unwrap();
        assert_eq!(deserialized.id, 111);
        assert_eq!(deserialized.name, "null_test");
        assert_eq!(deserialized.optional_field, None);
    }

    #[test]
    fn test_json_deserialize_invalid_data() {
        let serializer = Serializer::json();
        let invalid_json = b"invalid json data";

        let result: TransportResult<SimpleStruct> = serializer.deserialize(invalid_json);
        assert!(result.is_err());

        match result.unwrap_err() {
            TransportError::Serialization(msg) => {
                assert!(!msg.is_empty());
            }
            _ => panic!("Expected Serialization error"),
        }
    }

    #[test]
    fn test_json_deserialize_wrong_structure() {
        let serializer = Serializer::json();
        let wrong_structure = br#"{"wrong_field":"wrong_value"}"#;

        let result: TransportResult<SimpleStruct> = serializer.deserialize(wrong_structure);
        assert!(result.is_err());

        match result.unwrap_err() {
            TransportError::Serialization(msg) => {
                assert!(!msg.is_empty());
            }
            _ => panic!("Expected Serialization error"),
        }
    }

    #[test]
    fn test_round_trip_serialization() {
        let serializer = Serializer::json();
        let original = TestStruct {
            id: 777,
            name: "round_trip_test".to_string(),
            optional_field: Some("round_trip_optional".to_string()),
        };

        // Serialize
        let serialized = serializer.serialize(&original).unwrap();

        // Deserialize
        let deserialized: TestStruct = serializer.deserialize(&serialized).unwrap();

        // Should be identical
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_round_trip_with_none_field() {
        let serializer = Serializer::json();
        let original = TestStruct {
            id: 888,
            name: "none_round_trip".to_string(),
            optional_field: None,
        };

        // Serialize
        let serialized = serializer.serialize(&original).unwrap();

        // Deserialize
        let deserialized: TestStruct = serializer.deserialize(&serialized).unwrap();

        // Should be identical
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_messagepack_format_unsupported_serialize() {
        let mut serializer = Serializer::json();
        serializer.format = SerializationFormat::MessagePack;

        let test_data = SimpleStruct { value: 123 };
        let result = serializer.serialize(&test_data);

        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::Configuration(msg) => {
                assert_eq!(msg, "Unsupported serialization format");
            }
            _ => panic!("Expected Configuration error"),
        }
    }

    #[test]
    fn test_messagepack_format_unsupported_deserialize() {
        let mut serializer = Serializer::json();
        serializer.format = SerializationFormat::MessagePack;

        let data = b"some data";
        let result: TransportResult<SimpleStruct> = serializer.deserialize(data);

        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::Configuration(msg) => {
                assert_eq!(msg, "Unsupported serialization format");
            }
            _ => panic!("Expected Configuration error"),
        }
    }

    #[test]
    fn test_protobuf_format_unsupported_serialize() {
        let mut serializer = Serializer::json();
        serializer.format = SerializationFormat::Protobuf;

        let test_data = SimpleStruct { value: 456 };
        let result = serializer.serialize(&test_data);

        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::Configuration(msg) => {
                assert_eq!(msg, "Unsupported serialization format");
            }
            _ => panic!("Expected Configuration error"),
        }
    }

    #[test]
    fn test_protobuf_format_unsupported_deserialize() {
        let mut serializer = Serializer::json();
        serializer.format = SerializationFormat::Protobuf;

        let data = b"some data";
        let result: TransportResult<SimpleStruct> = serializer.deserialize(data);

        assert!(result.is_err());
        match result.unwrap_err() {
            TransportError::Configuration(msg) => {
                assert_eq!(msg, "Unsupported serialization format");
            }
            _ => panic!("Expected Configuration error"),
        }
    }

    #[test]
    fn test_empty_data_deserialization() {
        let serializer = Serializer::json();
        let empty_data = b"";

        let result: TransportResult<SimpleStruct> = serializer.deserialize(empty_data);
        assert!(result.is_err());

        match result.unwrap_err() {
            TransportError::Serialization(_) => (),
            _ => panic!("Expected Serialization error"),
        }
    }

    #[test]
    fn test_large_data_serialization() {
        let serializer = Serializer::json();
        let large_string = "x".repeat(10000);
        let large_data = TestStruct {
            id: 1,
            name: large_string.clone(),
            optional_field: Some(large_string.clone()),
        };

        let result = serializer.serialize(&large_data);
        assert!(result.is_ok());

        let bytes = result.unwrap();
        assert!(bytes.len() > 10000); // Should be quite large

        // Test round trip
        let deserialized: TestStruct = serializer.deserialize(&bytes).unwrap();
        assert_eq!(deserialized, large_data);
    }
}
