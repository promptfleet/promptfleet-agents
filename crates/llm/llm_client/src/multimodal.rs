use crate::{
    error::LlmError,
    types::{ChatContentPart, LlmRequest},
};
use protocol_transport_core::ProtocolError;

const MAX_MULTIMODAL_PART_BYTES: usize = 50_000_000;
const MAX_MULTIMODAL_AGGREGATE_BYTES: usize = 50_000_000;
const MAX_FILE_PARTS: usize = 5;
const MAX_IMAGE_PARTS: usize = 5;

const ALLOWED_FILE_MIME_TYPES: &[&str] = &[
    "application/pdf",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "text/plain",
    "text/markdown",
];
const ALLOWED_IMAGE_MIME_TYPES: &[&str] =
    &["image/png", "image/jpeg", "image/webp", "image/gif"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileDetailPolicy {
    PdfOnly,
    Unsupported,
}

#[derive(Clone, Copy)]
struct MultimodalLimits {
    part_bytes: usize,
    aggregate_bytes: usize,
    file_parts: usize,
    image_parts: usize,
}

const STANDARD_LIMITS: MultimodalLimits = MultimodalLimits {
    part_bytes: MAX_MULTIMODAL_PART_BYTES,
    aggregate_bytes: MAX_MULTIMODAL_AGGREGATE_BYTES,
    file_parts: MAX_FILE_PARTS,
    image_parts: MAX_IMAGE_PARTS,
};

pub(crate) fn validate_multimodal_inputs(
    request: &LlmRequest,
    file_detail_policy: FileDetailPolicy,
) -> Result<(), LlmError> {
    validate_multimodal_inputs_with_limits(request, file_detail_policy, STANDARD_LIMITS)
}

fn validate_multimodal_inputs_with_limits(
    request: &LlmRequest,
    file_detail_policy: FileDetailPolicy,
    limits: MultimodalLimits,
) -> Result<(), LlmError> {
    let mut file_parts = 0usize;
    let mut image_parts = 0usize;
    let mut aggregate_bytes = 0usize;

    for part in request
        .messages
        .iter()
        .filter_map(|message| message.content_parts.as_ref())
        .flatten()
    {
        let decoded_bytes = match part {
            ChatContentPart::FileBase64 {
                filename,
                media_type,
                data,
                detail,
            } => {
                file_parts = file_parts
                    .checked_add(1)
                    .ok_or_else(|| invalid_input("file input count overflow"))?;
                if file_parts > limits.file_parts {
                    return Err(invalid_input(format!(
                        "multimodal requests support at most {} file inputs",
                        limits.file_parts
                    )));
                }
                if filename.trim().is_empty() || filename.contains('\0') {
                    return Err(invalid_input(
                        "file input filename must be non-empty and contain no NUL bytes",
                    ));
                }
                require_allowed_mime("file", media_type, ALLOWED_FILE_MIME_TYPES)?;
                validate_file_detail(detail.as_deref(), media_type, file_detail_policy)?;
                canonical_base64_decoded_len("file", data)?
            }
            ChatContentPart::ImageBase64 {
                media_type,
                data,
                detail,
            } => {
                image_parts = image_parts
                    .checked_add(1)
                    .ok_or_else(|| invalid_input("image input count overflow"))?;
                if image_parts > limits.image_parts {
                    return Err(invalid_input(format!(
                        "multimodal requests support at most {} image inputs",
                        limits.image_parts
                    )));
                }
                require_allowed_mime("image", media_type, ALLOWED_IMAGE_MIME_TYPES)?;
                validate_detail("image", detail.as_deref())?;
                canonical_base64_decoded_len("image", data)?
            }
            _ => continue,
        };

        if decoded_bytes >= limits.part_bytes {
            return Err(invalid_input(format!(
                "each multimodal input must be smaller than {} bytes",
                limits.part_bytes
            )));
        }
        aggregate_bytes = aggregate_bytes
            .checked_add(decoded_bytes)
            .ok_or_else(|| invalid_input("combined multimodal input size overflow"))?;
        if aggregate_bytes > limits.aggregate_bytes {
            return Err(invalid_input(format!(
                "combined multimodal inputs must not exceed {} bytes",
                limits.aggregate_bytes
            )));
        }
    }

    Ok(())
}

fn require_allowed_mime(
    input_kind: &str,
    media_type: &str,
    allowed: &[&str],
) -> Result<(), LlmError> {
    if allowed.contains(&media_type) {
        return Ok(());
    }
    Err(invalid_input(format!(
        "{input_kind} input MIME type {media_type:?} is not a supported canonical MIME type"
    )))
}

fn validate_file_detail(
    detail: Option<&str>,
    media_type: &str,
    policy: FileDetailPolicy,
) -> Result<(), LlmError> {
    let Some(detail) = detail else {
        return Ok(());
    };
    validate_detail("file", Some(detail))?;
    if media_type != "application/pdf" {
        return Err(invalid_input(
            "file detail is supported only for PDF inputs",
        ));
    }
    if policy == FileDetailPolicy::Unsupported {
        return Err(invalid_input(
            "file detail is not supported by this provider request mode",
        ));
    }
    Ok(())
}

fn validate_detail(input_kind: &str, detail: Option<&str>) -> Result<(), LlmError> {
    if detail.is_none_or(|detail| matches!(detail, "auto" | "low" | "high")) {
        return Ok(());
    }
    Err(invalid_input(format!(
        "{input_kind} detail must be one of auto, low, or high"
    )))
}

fn canonical_base64_decoded_len(input_kind: &str, data: &str) -> Result<usize, LlmError> {
    let bytes = data.as_bytes();
    if bytes.is_empty() {
        return Err(invalid_input(format!(
            "{input_kind} Base64 data must not be empty"
        )));
    }
    if bytes.len() % 4 != 0 {
        return Err(invalid_input(format!(
            "{input_kind} Base64 data must use RFC 4648 standard encoding"
        )));
    }

    let padding = if bytes.ends_with(b"==") {
        2
    } else if bytes.ends_with(b"=") {
        1
    } else {
        0
    };
    let content_len = bytes.len() - padding;
    let content = &bytes[..content_len];
    if content.is_empty()
        || content
            .iter()
            .any(|byte| standard_base64_value(*byte).is_none())
    {
        return Err(invalid_input(format!(
            "{input_kind} data is not canonical RFC 4648 standard Base64"
        )));
    }
    let last_value = standard_base64_value(content[content.len() - 1])
        .expect("content was validated as standard Base64");
    if bytes[content_len..].iter().any(|byte| *byte != b'=')
        || (padding == 1 && last_value & 0b0000_0011 != 0)
        || (padding == 2 && last_value & 0b0000_1111 != 0)
    {
        return Err(invalid_input(format!(
            "{input_kind} data is not canonical RFC 4648 standard Base64"
        )));
    }

    Ok((bytes.len() / 4) * 3 - padding)
}

fn standard_base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn invalid_input(message: impl Into<String>) -> LlmError {
    LlmError::Protocol(ProtocolError::Validation(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatMessage, LlmRequest};

    fn request(parts: Vec<ChatContentPart>) -> LlmRequest {
        LlmRequest {
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content_parts: Some(parts),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    #[test]
    fn test_validate_multimodal_inputs_accepts_supported_image_and_file() {
        let request = request(vec![
            ChatContentPart::image_base64("image/png", "aW1hZ2U=", Some("high".to_string())),
            ChatContentPart::file_base64(
                "incident.pdf",
                "application/pdf",
                "JVBERi0xLjQ=",
                Some("auto".to_string()),
            ),
        ]);

        validate_multimodal_inputs(&request, FileDetailPolicy::PdfOnly)
            .expect("supported multimodal input should validate");
    }

    #[test]
    fn test_validate_multimodal_inputs_preserves_text_only_requests() {
        let request = request(vec![ChatContentPart::text("hello")]);

        validate_multimodal_inputs(&request, FileDetailPolicy::Unsupported)
            .expect("text-only requests should not be restricted");
    }

    #[test]
    fn test_validate_multimodal_inputs_rejects_noncanonical_base64_padding_bits() {
        let request = request(vec![ChatContentPart::image_base64(
            "image/png",
            "AB==",
            None,
        )]);

        let error = validate_multimodal_inputs(&request, FileDetailPolicy::PdfOnly)
            .expect_err("non-zero unused Base64 bits must be rejected");
        assert!(error.to_string().contains("canonical"));
    }

    #[test]
    fn test_validate_multimodal_inputs_rejects_aggregate_overflow() {
        let request = request(vec![
            ChatContentPart::image_base64("image/png", "YQ==", None),
            ChatContentPart::file_base64("notes.txt", "text/plain", "Yg==", None),
        ]);
        let limits = MultimodalLimits {
            part_bytes: 2,
            aggregate_bytes: 1,
            file_parts: 5,
            image_parts: 5,
        };

        let error = validate_multimodal_inputs_with_limits(
            &request,
            FileDetailPolicy::PdfOnly,
            limits,
        )
        .expect_err("combined decoded bytes above the aggregate limit must fail");
        assert!(error.to_string().contains("combined multimodal"));
    }

    #[test]
    fn test_validate_multimodal_inputs_rejects_part_at_limit() {
        let request = request(vec![ChatContentPart::image_base64(
            "image/png",
            "YWJj",
            None,
        )]);
        let limits = MultimodalLimits {
            part_bytes: 3,
            aggregate_bytes: 10,
            file_parts: 5,
            image_parts: 5,
        };

        let error = validate_multimodal_inputs_with_limits(
            &request,
            FileDetailPolicy::PdfOnly,
            limits,
        )
        .expect_err("a decoded part at the strict byte limit must fail");
        assert!(error.to_string().contains("each multimodal input"));
    }

    #[test]
    fn test_validate_multimodal_inputs_rejects_file_count_over_limit() {
        let request = request(
            (0..=MAX_FILE_PARTS)
                .map(|index| {
                    ChatContentPart::file_base64(
                        format!("notes-{index}.txt"),
                        "text/plain",
                        "YQ==",
                        None,
                    )
                })
                .collect(),
        );

        let error = validate_multimodal_inputs(&request, FileDetailPolicy::PdfOnly)
            .expect_err("file count above the limit must fail");
        assert!(error.to_string().contains("file inputs"));
    }

    #[test]
    fn test_validate_multimodal_inputs_rejects_image_count_over_limit() {
        let request = request(
            (0..=MAX_IMAGE_PARTS)
                .map(|_| ChatContentPart::image_base64("image/png", "YQ==", None))
                .collect(),
        );

        let error = validate_multimodal_inputs(&request, FileDetailPolicy::PdfOnly)
            .expect_err("image count above the limit must fail");
        assert!(error.to_string().contains("image inputs"));
    }
}
