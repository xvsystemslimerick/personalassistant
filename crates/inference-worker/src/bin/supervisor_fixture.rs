//! Test fixture for subprocess lifecycle integration. This binary is never
//! bundled in the application.

use inference_worker::{read_frame, write_frame, ErrorCode, Request, Response, PROTOCOL_VERSION};
use std::{io, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = std::env::var("PA_SUPERVISOR_FIXTURE_MODE").unwrap_or_else(|_| "normal".into());
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    while let Some(request) = read_frame::<Request>(&mut input)? {
        let request_id = match request {
            Request::Health { request_id, .. } => {
                write_frame(
                    &mut output,
                    &Response::Ready {
                        protocol_version: PROTOCOL_VERSION,
                        request_id,
                    },
                )?;
                continue;
            }
            Request::Extract { request_id, .. } => request_id,
            Request::Cancel { request_id, .. } => request_id,
            Request::Shutdown { request_id, .. } => {
                write_frame(
                    &mut output,
                    &Response::Stopped {
                        protocol_version: PROTOCOL_VERSION,
                        request_id,
                    },
                )?;
                return Ok(());
            }
        };
        match mode.as_str() {
            "crash" => std::process::exit(17),
            "delay" => std::thread::sleep(Duration::from_secs(5)),
            _ => {}
        }
        write_frame(
            &mut output,
            &Response::Error {
                protocol_version: PROTOCOL_VERSION,
                request_id,
                code: ErrorCode::BackendUnavailable,
            },
        )?;
    }
    Ok(())
}
