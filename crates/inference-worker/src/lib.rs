//! Private, listener-free IPC contract for the isolated local inference worker.
//!
//! Frames are a four-byte big-endian length followed by UTF-8 JSON. The protocol
//! intentionally carries no provider credentials and has no network transport.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{self, Read, Write};
use thiserror::Error;
use zeroize::Zeroizing;

pub mod abi;
#[cfg(feature = "persistent-backend-experimental")]
pub mod backend;
pub mod process_adapter;
pub mod supervisor;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 128 * 1024;
pub const MAX_REQUEST_ID_BYTES: usize = 64;

/// Private source text with redacted diagnostics and deterministic memory
/// scrubbing when the last owner is dropped.
pub struct SensitiveString(Zeroizing<String>);

impl SensitiveString {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl From<String> for SensitiveString {
    fn from(value: String) -> Self {
        Self(Zeroizing::new(value))
    }
}

impl From<&str> for SensitiveString {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

impl Clone for SensitiveString {
    fn clone(&self) -> Self {
        Self::from(self.as_str().to_owned())
    }
}

impl std::fmt::Debug for SensitiveString {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SensitiveString")
            .field("bytes", &self.0.len())
            .finish()
    }
}

impl PartialEq for SensitiveString {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for SensitiveString {}

impl Serialize for SensitiveString {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SensitiveString {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(Self::from)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Health {
        protocol_version: u16,
        request_id: String,
    },
    Extract {
        protocol_version: u16,
        request_id: String,
        source: SensitiveString,
        deadline_unix_ms: u64,
    },
    Cancel {
        protocol_version: u16,
        request_id: String,
    },
    Shutdown {
        protocol_version: u16,
        request_id: String,
    },
}

impl Request {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let (version, id) = match self {
            Self::Health {
                protocol_version,
                request_id,
            }
            | Self::Extract {
                protocol_version,
                request_id,
                ..
            }
            | Self::Cancel {
                protocol_version,
                request_id,
            }
            | Self::Shutdown {
                protocol_version,
                request_id,
            } => (*protocol_version, request_id),
        };
        if version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion);
        }
        if id.is_empty()
            || id.len() > MAX_REQUEST_ID_BYTES
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ProtocolError::InvalidRequestId);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Ready {
        protocol_version: u16,
        request_id: String,
    },
    Extracted {
        protocol_version: u16,
        request_id: String,
        payload: serde_json::Value,
    },
    Cancelled {
        protocol_version: u16,
        request_id: String,
    },
    Error {
        protocol_version: u16,
        request_id: String,
        code: ErrorCode,
    },
    Stopped {
        protocol_version: u16,
        request_id: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedVersion,
    QueueFull,
    DeadlineExceeded,
    Cancelled,
    BackendUnavailable,
    InvalidOutput,
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("worker I/O failed")]
    Io(#[from] io::Error),
    #[error("frame exceeded the protocol limit")]
    FrameTooLarge,
    #[error("frame was not valid protocol JSON")]
    InvalidJson,
    #[error("unsupported worker protocol version")]
    UnsupportedVersion,
    #[error("invalid request identifier")]
    InvalidRequestId,
}

pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>, ProtocolError> {
    let mut header = [0_u8; 4];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    let mut body = Zeroizing::new(vec![0_u8; length]);
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|_| ProtocolError::InvalidJson)
}

pub fn write_frame<T: Serialize>(writer: &mut impl Write, value: &T) -> Result<(), ProtocolError> {
    let body = Zeroizing::new(serde_json::to_vec(value).map_err(|_| ProtocolError::InvalidJson)?);
    if body.is_empty() || body.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::FrameTooLarge);
    }
    writer.write_all(&(body.len() as u32).to_be_bytes())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

/// Runs the worker protocol over anonymous pipes. Extract requests remain
/// disabled until the pinned llama.cpp ABI backend is linked.
pub fn serve(reader: &mut impl Read, writer: &mut impl Write) -> Result<(), ProtocolError> {
    while let Some(request) = read_frame::<Request>(reader)? {
        let request_id = request_id(&request).to_owned();
        if let Err(error) = request.validate() {
            let code = match error {
                ProtocolError::UnsupportedVersion => ErrorCode::UnsupportedVersion,
                _ => ErrorCode::InvalidRequest,
            };
            write_frame(
                writer,
                &Response::Error {
                    protocol_version: PROTOCOL_VERSION,
                    request_id,
                    code,
                },
            )?;
            continue;
        }
        let response = match request {
            Request::Health { .. } => Response::Ready {
                protocol_version: PROTOCOL_VERSION,
                request_id,
            },
            Request::Extract { .. } => Response::Error {
                protocol_version: PROTOCOL_VERSION,
                request_id,
                code: ErrorCode::BackendUnavailable,
            },
            Request::Cancel { .. } => Response::Cancelled {
                protocol_version: PROTOCOL_VERSION,
                request_id,
            },
            Request::Shutdown { .. } => {
                write_frame(
                    writer,
                    &Response::Stopped {
                        protocol_version: PROTOCOL_VERSION,
                        request_id,
                    },
                )?;
                return Ok(());
            }
        };
        write_frame(writer, &response)?;
    }
    Ok(())
}

#[cfg(feature = "persistent-backend-experimental")]
pub fn serve_persistent(
    reader: &mut impl Read,
    writer: &mut impl Write,
    backend: &mut backend::PersistentBackend,
) -> Result<(), ProtocolError> {
    while let Some(request) = read_frame::<Request>(reader)? {
        let request_id = request_id(&request).to_owned();
        if let Err(error) = request.validate() {
            let code = match error {
                ProtocolError::UnsupportedVersion => ErrorCode::UnsupportedVersion,
                _ => ErrorCode::InvalidRequest,
            };
            write_frame(
                writer,
                &Response::Error {
                    protocol_version: PROTOCOL_VERSION,
                    request_id,
                    code,
                },
            )?;
            continue;
        }
        let response = match request {
            Request::Health { .. } => Response::Ready {
                protocol_version: PROTOCOL_VERSION,
                request_id,
            },
            Request::Extract {
                source,
                deadline_unix_ms,
                ..
            } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                if deadline_unix_ms <= now {
                    Response::Error {
                        protocol_version: PROTOCOL_VERSION,
                        request_id,
                        code: ErrorCode::DeadlineExceeded,
                    }
                } else if ai::contains_prompt_injection_signal(source.as_str()) {
                    Response::Error {
                        protocol_version: PROTOCOL_VERSION,
                        request_id,
                        code: ErrorCode::InvalidOutput,
                    }
                } else {
                    match backend.extract(source.as_str()) {
                        Ok(extraction) => Response::Extracted {
                            protocol_version: PROTOCOL_VERSION,
                            request_id,
                            payload: serde_json::to_value(extraction)
                                .unwrap_or(serde_json::Value::Null),
                        },
                        Err(_) => Response::Error {
                            protocol_version: PROTOCOL_VERSION,
                            request_id,
                            code: ErrorCode::InvalidOutput,
                        },
                    }
                }
            }
            Request::Cancel { .. } => Response::Cancelled {
                protocol_version: PROTOCOL_VERSION,
                request_id,
            },
            Request::Shutdown { .. } => {
                write_frame(
                    writer,
                    &Response::Stopped {
                        protocol_version: PROTOCOL_VERSION,
                        request_id,
                    },
                )?;
                return Ok(());
            }
        };
        write_frame(writer, &response)?;
    }
    Ok(())
}

fn request_id(request: &Request) -> &str {
    match request {
        Request::Health { request_id, .. }
        | Request::Extract { request_id, .. }
        | Request::Cancel { request_id, .. }
        | Request::Shutdown { request_id, .. } => request_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_through_a_frame() {
        let request = Request::Health {
            protocol_version: PROTOCOL_VERSION,
            request_id: "health_1".into(),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &request).unwrap();
        let decoded: Request = read_frame(&mut bytes.as_slice()).unwrap().unwrap();
        assert_eq!(decoded, request);
        decoded.validate().unwrap();
    }

    #[test]
    fn oversized_inbound_frame_is_rejected_before_allocation() {
        let bytes = ((MAX_FRAME_BYTES as u32) + 1).to_be_bytes();
        assert!(matches!(
            read_frame::<Request>(&mut bytes.as_slice()),
            Err(ProtocolError::FrameTooLarge)
        ));
    }

    #[test]
    fn invalid_version_and_identifier_are_rejected() {
        let wrong_version = Request::Health {
            protocol_version: PROTOCOL_VERSION + 1,
            request_id: "health".into(),
        };
        assert!(matches!(
            wrong_version.validate(),
            Err(ProtocolError::UnsupportedVersion)
        ));
        let unsafe_id = Request::Health {
            protocol_version: PROTOCOL_VERSION,
            request_id: "../../secret".into(),
        };
        assert!(matches!(
            unsafe_id.validate(),
            Err(ProtocolError::InvalidRequestId)
        ));
    }

    #[test]
    fn truncated_body_is_an_io_failure() {
        let mut bytes = 10_u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"short");
        assert!(matches!(
            read_frame::<Request>(&mut bytes.as_slice()),
            Err(ProtocolError::Io(_))
        ));
    }

    #[test]
    fn extract_request_debug_redacts_sensitive_source() {
        let request = Request::Extract {
            protocol_version: PROTOCOL_VERSION,
            request_id: "private_1".into(),
            source: "private provider body".into(),
            deadline_unix_ms: 100,
        };
        let debug = format!("{request:?}");
        assert!(debug.contains("private_1"));
        assert!(!debug.contains("private provider body"));
    }

    #[test]
    fn worker_handles_health_then_clean_shutdown_without_a_listener() {
        let requests = [
            Request::Health {
                protocol_version: PROTOCOL_VERSION,
                request_id: "h1".into(),
            },
            Request::Shutdown {
                protocol_version: PROTOCOL_VERSION,
                request_id: "s1".into(),
            },
        ];
        let mut input = Vec::new();
        for request in requests {
            write_frame(&mut input, &request).unwrap();
        }
        let mut output = Vec::new();
        serve(&mut input.as_slice(), &mut output).unwrap();
        let mut output = output.as_slice();
        assert!(matches!(
            read_frame::<Response>(&mut output).unwrap(),
            Some(Response::Ready { .. })
        ));
        assert!(matches!(
            read_frame::<Response>(&mut output).unwrap(),
            Some(Response::Stopped { .. })
        ));
        assert!(read_frame::<Response>(&mut output).unwrap().is_none());
    }

    #[test]
    fn extraction_is_fail_closed_until_backend_is_linked() {
        let request = Request::Extract {
            protocol_version: PROTOCOL_VERSION,
            request_id: "e1".into(),
            source: "private fixture".to_owned().into(),
            deadline_unix_ms: 1,
        };
        let mut input = Vec::new();
        write_frame(&mut input, &request).unwrap();
        let mut output = Vec::new();
        serve(&mut input.as_slice(), &mut output).unwrap();
        assert_eq!(
            read_frame::<Response>(&mut output.as_slice())
                .unwrap()
                .unwrap(),
            Response::Error {
                protocol_version: PROTOCOL_VERSION,
                request_id: "e1".into(),
                code: ErrorCode::BackendUnavailable,
            }
        );
    }
}
