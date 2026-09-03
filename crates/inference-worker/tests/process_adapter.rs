use inference_worker::{
    process_adapter::{AdapterError, AdapterEvent, ProcessAdapter, ProcessConfig},
    supervisor::{Job, SubmitOutcome},
};
use std::time::{Duration, Instant};

fn fixture(mode: &str) -> ProcessAdapter {
    ProcessAdapter::new(
        ProcessConfig::new(env!("CARGO_BIN_EXE_supervisor_fixture"))
            .env("PA_SUPERVISOR_FIXTURE_MODE", mode),
    )
}

fn job(id: &str, deadline: u64) -> Job {
    Job {
        request_id: id.into(),
        deadline_unix_ms: deadline,
    }
}

fn wait_for_event(adapter: &mut ProcessAdapter, now: u64) -> Result<AdapterEvent, AdapterError> {
    let started = Instant::now();
    loop {
        if let Some(event) = adapter.poll(now)? {
            return Ok(event);
        }
        assert!(started.elapsed() < Duration::from_secs(2));
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn subprocess_completes_active_then_promotes_fifo_work() {
    let mut adapter = fixture("normal");
    assert_eq!(
        adapter.submit(job("first", 1_000), "private first".into(), 0),
        Ok(SubmitOutcome::Started)
    );
    assert_eq!(
        adapter.submit(job("second", 1_000), "private second".into(), 0),
        Ok(SubmitOutcome::Queued)
    );
    assert!(matches!(
        wait_for_event(&mut adapter, 1).unwrap(),
        AdapterEvent::Completed { request_id, .. } if request_id == "first"
    ));
    assert!(matches!(
        wait_for_event(&mut adapter, 2).unwrap(),
        AdapterEvent::Completed { request_id, .. } if request_id == "second"
    ));
}

#[test]
fn active_cancellation_kills_worker_and_promotes_queued_work() {
    let mut adapter = fixture("delay");
    adapter
        .submit(job("cancel_me", 10_000), "private".into(), 0)
        .unwrap();
    adapter
        .submit(job("next", 10_000), "private next".into(), 0)
        .unwrap();
    let started = Instant::now();
    assert_eq!(
        adapter.cancel("cancel_me", 1).unwrap(),
        Some(AdapterEvent::Cancelled {
            request_id: "cancel_me".into()
        })
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(adapter.pending_len(), 0);
}

#[test]
fn deadline_terminates_blocked_worker_without_waiting_for_output() {
    let mut adapter = fixture("delay");
    adapter
        .submit(job("expires", 10), "private".into(), 0)
        .unwrap();
    let started = Instant::now();
    assert_eq!(
        adapter.poll(10).unwrap(),
        Some(AdapterEvent::DeadlineExceeded {
            request_id: "expires".into()
        })
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn repeated_crashes_exhaust_the_three_restart_budget() {
    let mut adapter = fixture("crash");
    for index in 0..5 {
        let outcome = adapter
            .submit(job(&format!("crash_{index}"), 10_000), "private".into(), 0)
            .unwrap();
        assert_eq!(
            outcome,
            if index == 0 {
                SubmitOutcome::Started
            } else {
                SubmitOutcome::Queued
            }
        );
    }

    for _ in 0..3 {
        assert!(matches!(
            wait_for_event(&mut adapter, 1).unwrap(),
            AdapterEvent::WorkerFailed { .. }
        ));
    }
    assert_eq!(
        wait_for_event(&mut adapter, 1),
        Err(AdapterError::RestartBudgetExhausted)
    );
}
