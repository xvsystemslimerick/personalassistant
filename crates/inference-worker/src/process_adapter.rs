//! Desktop-owned process isolation for private inference work.
//!
//! This adapter deliberately uses anonymous pipes, suppresses worker stderr, and
//! kills the entire worker on active cancellation or deadline expiry. A killed
//! worker is never reused, so a late result cannot cross a cancellation boundary.

use crate::{
    read_frame,
    supervisor::{CancelOutcome, Job, PrivateWork, PrivateWorkQueue, SubmitOutcome},
    write_frame, Request, Response, SensitiveString, PROTOCOL_VERSION,
};
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, TryRecvError},
};
use thiserror::Error;

pub const RESTART_WINDOW_MS: u64 = 60_000;
pub const MAX_RESTARTS_PER_WINDOW: u8 = 3;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdapterError {
    #[error("the inference worker could not start")]
    StartFailed,
    #[error("the inference worker pipe is unavailable")]
    PipeUnavailable,
    #[error("the inference worker protocol failed")]
    ProtocolFailed,
    #[error("the inference worker restart budget is exhausted")]
    RestartBudgetExhausted,
}

#[derive(Debug, PartialEq)]
pub enum AdapterEvent {
    Completed {
        request_id: String,
        response: Response,
    },
    DeadlineExceeded {
        request_id: String,
    },
    Cancelled {
        request_id: String,
    },
    WorkerFailed {
        request_id: String,
    },
}

#[derive(Clone, Debug)]
pub struct ProcessConfig {
    executable: PathBuf,
    environment: Vec<(OsString, OsString)>,
}

impl ProcessConfig {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            environment: Vec::new(),
        }
    }

    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.environment
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }
}

struct WorkerProcess {
    child: Child,
    input: ChildStdin,
    responses: Receiver<Result<Option<Response>, crate::ProtocolError>>,
}

impl WorkerProcess {
    fn spawn(config: &ProcessConfig) -> Result<Self, AdapterError> {
        let mut command = Command::new(&config.executable);
        command
            .envs(config.environment.iter().cloned())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|_| AdapterError::StartFailed)?;
        let input = child.stdin.take().ok_or(AdapterError::PipeUnavailable)?;
        let mut output = child.stdout.take().ok_or(AdapterError::PipeUnavailable)?;
        let (sender, responses) = mpsc::channel();
        std::thread::spawn(move || loop {
            let response = read_frame::<Response>(&mut output);
            let finished = !matches!(response, Ok(Some(_)));
            if sender.send(response).is_err() || finished {
                break;
            }
        });
        Ok(Self {
            child,
            input,
            responses,
        })
    }

    fn terminate(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Owns the private queue and the single isolated worker process.
///
/// `Debug` is intentionally manual and never includes source payloads or
/// environment values (which may include private filesystem paths).
pub struct ProcessAdapter {
    config: ProcessConfig,
    queue: PrivateWorkQueue<SensitiveString>,
    worker: Option<WorkerProcess>,
    active_dispatched: bool,
    restart_window_started_ms: Option<u64>,
    restarts_in_window: u8,
    restart_required: bool,
}

impl std::fmt::Debug for ProcessAdapter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessAdapter")
            .field("executable", &self.config.executable.file_name())
            .field("queue", &self.queue)
            .field("worker_running", &self.worker.is_some())
            .field("restarts_in_window", &self.restarts_in_window)
            .finish()
    }
}

impl ProcessAdapter {
    pub fn new(config: ProcessConfig) -> Self {
        Self {
            config,
            queue: PrivateWorkQueue::default(),
            worker: None,
            active_dispatched: false,
            restart_window_started_ms: None,
            restarts_in_window: 0,
            restart_required: false,
        }
    }

    pub fn submit(
        &mut self,
        job: Job,
        source: String,
        now_unix_ms: u64,
    ) -> Result<SubmitOutcome, AdapterError> {
        let outcome = self.queue.submit(
            PrivateWork {
                job,
                payload: source.into(),
            },
            now_unix_ms,
        );
        if outcome == SubmitOutcome::Started {
            if let Err(error) = self.dispatch_active(now_unix_ms) {
                let request_id = self
                    .queue
                    .active()
                    .expect("a newly started job remains active")
                    .job
                    .request_id
                    .clone();
                self.queue.cancel(&request_id);
                self.terminate_worker();
                return Err(error);
            }
        }
        Ok(outcome)
    }

    pub fn start_and_health_check(
        &mut self,
        request_id: &str,
        timeout: std::time::Duration,
        now_unix_ms: u64,
    ) -> Result<(), AdapterError> {
        if self.worker.is_none() {
            if self.restart_required {
                self.reserve_restart(now_unix_ms)?;
            }
            self.worker = Some(WorkerProcess::spawn(&self.config)?);
            self.restart_required = false;
        }
        let request = Request::Health {
            protocol_version: PROTOCOL_VERSION,
            request_id: request_id.into(),
        };
        let worker = self.worker.as_mut().unwrap();
        write_frame(&mut worker.input, &request).map_err(|_| AdapterError::ProtocolFailed)?;
        match worker.responses.recv_timeout(timeout) {
            Ok(Ok(Some(Response::Ready {
                request_id: response_id,
                ..
            }))) if response_id == request_id => Ok(()),
            _ => {
                self.terminate_worker();
                self.restart_required = true;
                Err(AdapterError::ProtocolFailed)
            }
        }
    }

    pub fn cancel(
        &mut self,
        request_id: &str,
        now_unix_ms: u64,
    ) -> Result<Option<AdapterEvent>, AdapterError> {
        match self.queue.cancel(request_id) {
            CancelOutcome::TerminateWorker => {
                self.terminate_worker();
                self.promote_and_dispatch(now_unix_ms)?;
                Ok(Some(AdapterEvent::Cancelled {
                    request_id: request_id.into(),
                }))
            }
            CancelOutcome::RemovedFromQueue | CancelOutcome::NotFound => Ok(None),
        }
    }

    /// Advances deadlines and consumes at most one worker response.
    pub fn poll(&mut self, now_unix_ms: u64) -> Result<Option<AdapterEvent>, AdapterError> {
        let Some(active) = self.queue.active() else {
            return Ok(None);
        };
        if active.job.deadline_unix_ms <= now_unix_ms {
            let request_id = active.job.request_id.clone();
            self.terminate_worker();
            self.queue.complete_active(now_unix_ms);
            self.dispatch_active(now_unix_ms)?;
            return Ok(Some(AdapterEvent::DeadlineExceeded { request_id }));
        }

        let response = match self.worker.as_mut() {
            Some(worker) => match worker.responses.try_recv() {
                Ok(Ok(Some(response))) => Some(Ok(response)),
                Ok(Ok(None)) | Err(TryRecvError::Disconnected) => Some(Err(())),
                Ok(Err(_)) => Some(Err(())),
                Err(TryRecvError::Empty) => {
                    if worker.child.try_wait().ok().flatten().is_some() {
                        Some(Err(()))
                    } else {
                        None
                    }
                }
            },
            None => Some(Err(())),
        };

        match response {
            Some(Ok(response)) => {
                let active_id = self.queue.active().unwrap().job.request_id.clone();
                if response_request_id(&response) != active_id {
                    self.handle_worker_failure(now_unix_ms)?;
                    return Err(AdapterError::ProtocolFailed);
                }
                self.queue.complete_active(now_unix_ms);
                self.active_dispatched = false;
                self.dispatch_active(now_unix_ms)?;
                Ok(Some(AdapterEvent::Completed {
                    request_id: active_id,
                    response,
                }))
            }
            Some(Err(())) => {
                let request_id = self.queue.active().unwrap().job.request_id.clone();
                self.handle_worker_failure(now_unix_ms)?;
                Ok(Some(AdapterEvent::WorkerFailed { request_id }))
            }
            None => Ok(None),
        }
    }

    pub fn pending_len(&self) -> usize {
        self.queue.pending_len()
    }

    fn promote_and_dispatch(&mut self, now_unix_ms: u64) -> Result<(), AdapterError> {
        self.queue.complete_active(now_unix_ms);
        self.active_dispatched = false;
        self.dispatch_active(now_unix_ms)
    }

    fn dispatch_active(&mut self, now_unix_ms: u64) -> Result<(), AdapterError> {
        if self.queue.active().is_none() || self.active_dispatched {
            return Ok(());
        }
        if self.worker.is_none() {
            if self.restart_required {
                self.reserve_restart(now_unix_ms)?;
            }
            self.worker = Some(WorkerProcess::spawn(&self.config)?);
            self.restart_required = false;
        }
        let work = self.queue.active().unwrap();
        let request = Request::Extract {
            protocol_version: PROTOCOL_VERSION,
            request_id: work.job.request_id.clone(),
            source: work.payload.clone(),
            deadline_unix_ms: work.job.deadline_unix_ms,
        };
        write_frame(&mut self.worker.as_mut().unwrap().input, &request)
            .map_err(|_| AdapterError::ProtocolFailed)?;
        self.active_dispatched = true;
        Ok(())
    }

    fn handle_worker_failure(&mut self, now_unix_ms: u64) -> Result<(), AdapterError> {
        self.terminate_worker();
        self.restart_required = true;
        self.queue.complete_active(now_unix_ms);
        self.active_dispatched = false;
        self.dispatch_active(now_unix_ms)
    }

    fn reserve_restart(&mut self, now_unix_ms: u64) -> Result<(), AdapterError> {
        if self
            .restart_window_started_ms
            .is_none_or(|start| now_unix_ms.saturating_sub(start) >= RESTART_WINDOW_MS)
        {
            self.restart_window_started_ms = Some(now_unix_ms);
            self.restarts_in_window = 0;
        }
        if self.restarts_in_window >= MAX_RESTARTS_PER_WINDOW {
            return Err(AdapterError::RestartBudgetExhausted);
        }
        self.restarts_in_window += 1;
        Ok(())
    }

    fn terminate_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.terminate();
        }
        self.active_dispatched = false;
    }
}

impl Drop for ProcessAdapter {
    fn drop(&mut self) {
        self.terminate_worker();
    }
}

fn response_request_id(response: &Response) -> &str {
    match response {
        Response::Ready { request_id, .. }
        | Response::Extracted { request_id, .. }
        | Response::Cancelled { request_id, .. }
        | Response::Error { request_id, .. }
        | Response::Stopped { request_id, .. } => request_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_budget_is_bounded_and_resets_after_window() {
        let mut adapter = ProcessAdapter::new(ProcessConfig::new("worker"));
        for _ in 0..MAX_RESTARTS_PER_WINDOW {
            adapter.reserve_restart(10).unwrap();
        }
        assert_eq!(
            adapter.reserve_restart(10),
            Err(AdapterError::RestartBudgetExhausted)
        );
        adapter.reserve_restart(10 + RESTART_WINDOW_MS).unwrap();
    }

    #[test]
    fn debug_redacts_sources_and_environment_values() {
        let mut adapter = ProcessAdapter::new(
            ProcessConfig::new("worker").env("PRIVATE_PATH", "/private/model/location"),
        );
        adapter.queue.submit(
            PrivateWork {
                job: Job {
                    request_id: "safe_id".into(),
                    deadline_unix_ms: 100,
                },
                payload: "private email body".into(),
            },
            0,
        );
        let debug = format!("{adapter:?}");
        assert!(debug.contains("safe_id"));
        assert!(!debug.contains("private email body"));
        assert!(!debug.contains("/private/model/location"));
    }

    #[test]
    fn response_ids_are_available_for_strict_correlation() {
        let response = Response::Ready {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request_1".into(),
        };
        assert_eq!(response_request_id(&response), "request_1");
    }

    #[test]
    fn restart_window_uses_saturating_time_math() {
        let mut adapter = ProcessAdapter::new(ProcessConfig::new("worker"));
        adapter.reserve_restart(100).unwrap();
        adapter.reserve_restart(50).unwrap();
        assert_eq!(adapter.restarts_in_window, 2);
    }

    #[test]
    fn missing_worker_fails_closed_without_retaining_a_process() {
        let mut adapter = ProcessAdapter::new(ProcessConfig::new(
            "/definitely/not/a/personal-assistant-worker",
        ));
        let result = adapter.submit(
            Job {
                request_id: "request_1".into(),
                deadline_unix_ms: 100,
            },
            "private source".into(),
            0,
        );
        assert_eq!(result, Err(AdapterError::StartFailed));
        assert!(adapter.worker.is_none());
    }

    #[test]
    fn duration_constant_is_intentionally_bounded() {
        assert_eq!(
            std::time::Duration::from_millis(RESTART_WINDOW_MS).as_secs(),
            60
        );
    }
}
