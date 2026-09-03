use std::collections::VecDeque;

pub const MAX_PENDING_JOBS: usize = 8;
pub const MAX_RESTARTS_PER_WINDOW: u8 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub request_id: String,
    pub deadline_unix_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmitOutcome {
    Started,
    Queued,
    QueueFull,
    DeadlineExceeded,
    Duplicate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CancelOutcome {
    TerminateWorker,
    RemovedFromQueue,
    NotFound,
}

/// Deterministic queue/restart policy owned by the desktop supervisor.
/// Process spawning and private payload transport remain outside this type.
#[derive(Debug, Default)]
pub struct SupervisorPolicy {
    active: Option<Job>,
    pending: VecDeque<Job>,
    restarts_in_window: u8,
}

impl SupervisorPolicy {
    pub fn submit(&mut self, job: Job, now_unix_ms: u64) -> SubmitOutcome {
        if job.deadline_unix_ms <= now_unix_ms {
            return SubmitOutcome::DeadlineExceeded;
        }
        if self.active.as_ref().map(|active| &active.request_id) == Some(&job.request_id)
            || self
                .pending
                .iter()
                .any(|pending| pending.request_id == job.request_id)
        {
            return SubmitOutcome::Duplicate;
        }
        if self.active.is_none() {
            self.active = Some(job);
            return SubmitOutcome::Started;
        }
        if self.pending.len() >= MAX_PENDING_JOBS {
            return SubmitOutcome::QueueFull;
        }
        self.pending.push_back(job);
        SubmitOutcome::Queued
    }

    pub fn complete_active(&mut self, now_unix_ms: u64) -> Option<Job> {
        self.active = None;
        while let Some(next) = self.pending.pop_front() {
            if next.deadline_unix_ms > now_unix_ms {
                self.active = Some(next.clone());
                return Some(next);
            }
        }
        None
    }

    pub fn cancel(&mut self, request_id: &str) -> CancelOutcome {
        if self.active.as_ref().map(|job| job.request_id.as_str()) == Some(request_id) {
            self.active = None;
            return CancelOutcome::TerminateWorker;
        }
        if let Some(index) = self
            .pending
            .iter()
            .position(|job| job.request_id == request_id)
        {
            self.pending.remove(index);
            return CancelOutcome::RemovedFromQueue;
        }
        CancelOutcome::NotFound
    }

    pub fn record_unexpected_exit(&mut self) -> bool {
        self.active = None;
        if self.restarts_in_window >= MAX_RESTARTS_PER_WINDOW {
            return false;
        }
        self.restarts_in_window += 1;
        true
    }

    pub fn reset_restart_window(&mut self) {
        self.restarts_in_window = 0;
    }

    pub fn active(&self) -> Option<&Job> {
        self.active.as_ref()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

/// Keeps private work payloads coupled to their sanitized queue metadata.
/// Payloads are intentionally excluded from `Debug` output.
pub struct PrivateWork<T> {
    pub job: Job,
    pub payload: T,
}

pub struct PrivateWorkQueue<T> {
    active: Option<PrivateWork<T>>,
    pending: VecDeque<PrivateWork<T>>,
}

impl<T> Default for PrivateWorkQueue<T> {
    fn default() -> Self {
        Self {
            active: None,
            pending: VecDeque::new(),
        }
    }
}

impl<T> std::fmt::Debug for PrivateWorkQueue<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateWorkQueue")
            .field(
                "active_request_id",
                &self.active.as_ref().map(|work| &work.job.request_id),
            )
            .field("pending_count", &self.pending.len())
            .finish()
    }
}

impl<T> PrivateWorkQueue<T> {
    pub fn submit(&mut self, work: PrivateWork<T>, now_unix_ms: u64) -> SubmitOutcome {
        if work.job.deadline_unix_ms <= now_unix_ms {
            return SubmitOutcome::DeadlineExceeded;
        }
        if self.contains(&work.job.request_id) {
            return SubmitOutcome::Duplicate;
        }
        if self.active.is_none() {
            self.active = Some(work);
            return SubmitOutcome::Started;
        }
        if self.pending.len() >= MAX_PENDING_JOBS {
            return SubmitOutcome::QueueFull;
        }
        self.pending.push_back(work);
        SubmitOutcome::Queued
    }

    pub fn active(&self) -> Option<&PrivateWork<T>> {
        self.active.as_ref()
    }

    pub fn active_mut(&mut self) -> Option<&mut PrivateWork<T>> {
        self.active.as_mut()
    }

    pub fn complete_active(&mut self, now_unix_ms: u64) -> Option<&PrivateWork<T>> {
        self.active = None;
        while let Some(next) = self.pending.pop_front() {
            if next.job.deadline_unix_ms > now_unix_ms {
                self.active = Some(next);
                return self.active.as_ref();
            }
        }
        None
    }

    pub fn cancel(&mut self, request_id: &str) -> CancelOutcome {
        if self
            .active
            .as_ref()
            .is_some_and(|work| work.job.request_id == request_id)
        {
            self.active = None;
            return CancelOutcome::TerminateWorker;
        }
        if let Some(index) = self
            .pending
            .iter()
            .position(|work| work.job.request_id == request_id)
        {
            self.pending.remove(index);
            return CancelOutcome::RemovedFromQueue;
        }
        CancelOutcome::NotFound
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    fn contains(&self, request_id: &str) -> bool {
        self.active
            .as_ref()
            .is_some_and(|work| work.job.request_id == request_id)
            || self
                .pending
                .iter()
                .any(|work| work.job.request_id == request_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, deadline: u64) -> Job {
        Job {
            request_id: id.into(),
            deadline_unix_ms: deadline,
        }
    }

    #[test]
    fn starts_one_and_bounds_pending_work() {
        let mut policy = SupervisorPolicy::default();
        assert_eq!(policy.submit(job("active", 100), 0), SubmitOutcome::Started);
        for index in 0..MAX_PENDING_JOBS {
            assert_eq!(
                policy.submit(job(&format!("queued_{index}"), 100), 0),
                SubmitOutcome::Queued
            );
        }
        assert_eq!(
            policy.submit(job("overflow", 100), 0),
            SubmitOutcome::QueueFull
        );
    }

    #[test]
    fn expired_jobs_never_start() {
        let mut policy = SupervisorPolicy::default();
        assert_eq!(
            policy.submit(job("expired", 10), 10),
            SubmitOutcome::DeadlineExceeded
        );
        policy.submit(job("active", 100), 0);
        policy.submit(job("stale", 5), 0);
        policy.submit(job("fresh", 50), 0);
        assert_eq!(policy.complete_active(10), Some(job("fresh", 50)));
    }

    #[test]
    fn active_cancellation_requires_process_termination() {
        let mut policy = SupervisorPolicy::default();
        policy.submit(job("active", 100), 0);
        assert_eq!(policy.cancel("active"), CancelOutcome::TerminateWorker);
        assert!(policy.active().is_none());
    }

    #[test]
    fn queued_cancellation_does_not_disturb_active_work() {
        let mut policy = SupervisorPolicy::default();
        policy.submit(job("active", 100), 0);
        policy.submit(job("queued", 100), 0);
        assert_eq!(policy.cancel("queued"), CancelOutcome::RemovedFromQueue);
        assert_eq!(policy.pending_len(), 0);
        assert_eq!(policy.active(), Some(&job("active", 100)));
    }

    #[test]
    fn crash_restarts_are_bounded() {
        let mut policy = SupervisorPolicy::default();
        for _ in 0..MAX_RESTARTS_PER_WINDOW {
            assert!(policy.record_unexpected_exit());
        }
        assert!(!policy.record_unexpected_exit());
        policy.reset_restart_window();
        assert!(policy.record_unexpected_exit());
    }

    #[test]
    fn duplicate_request_ids_are_rejected() {
        let mut policy = SupervisorPolicy::default();
        assert_eq!(policy.submit(job("same", 100), 0), SubmitOutcome::Started);
        assert_eq!(policy.submit(job("same", 200), 0), SubmitOutcome::Duplicate);
        policy.submit(job("queued", 100), 0);
        assert_eq!(
            policy.submit(job("queued", 200), 0),
            SubmitOutcome::Duplicate
        );
    }

    #[test]
    fn private_payloads_follow_fifo_promotion_and_expiry() {
        let mut queue = PrivateWorkQueue::default();
        queue.submit(
            PrivateWork {
                job: job("active", 100),
                payload: "active secret",
            },
            0,
        );
        queue.submit(
            PrivateWork {
                job: job("stale", 5),
                payload: "stale secret",
            },
            0,
        );
        queue.submit(
            PrivateWork {
                job: job("fresh", 100),
                payload: "fresh secret",
            },
            0,
        );
        let promoted = queue.complete_active(10).unwrap();
        assert_eq!(promoted.job.request_id, "fresh");
        assert_eq!(promoted.payload, "fresh secret");
        assert_eq!(queue.pending_len(), 0);
    }

    #[test]
    fn private_queue_debug_never_contains_payloads() {
        let mut queue = PrivateWorkQueue::default();
        queue.submit(
            PrivateWork {
                job: job("active", 100),
                payload: "private email body",
            },
            0,
        );
        let debug = format!("{queue:?}");
        assert!(debug.contains("active"));
        assert!(!debug.contains("private email body"));
    }

    #[test]
    fn private_queue_cancellation_removes_the_matching_payload_only() {
        let mut queue = PrivateWorkQueue::default();
        queue.submit(
            PrivateWork {
                job: job("active", 100),
                payload: "active",
            },
            0,
        );
        queue.submit(
            PrivateWork {
                job: job("queued", 100),
                payload: "queued",
            },
            0,
        );
        assert_eq!(queue.cancel("queued"), CancelOutcome::RemovedFromQueue);
        assert_eq!(queue.active().unwrap().payload, "active");
        assert_eq!(queue.pending_len(), 0);
    }
}
