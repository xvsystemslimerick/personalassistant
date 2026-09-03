use chrono::{DateTime, Duration, FixedOffset, Timelike};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    Reminder,
    UrgentAlert,
    MorningSummary,
    EveningSummary,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrivacyClass {
    PublicFamily,
    Private,
    WorkPrivate,
    Sensitive,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentMode {
    Full,
    Generic,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuietHours {
    pub start_minute: u16,
    pub end_minute: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationRequest {
    pub idempotency_key: String,
    pub kind: NotificationKind,
    pub privacy: PrivacyClass,
    pub content_mode: ContentMode,
    pub title: String,
    pub body: String,
    pub deliver_at: DateTime<FixedOffset>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPlan {
    pub idempotency_key: String,
    pub kind: NotificationKind,
    pub privacy: PrivacyClass,
    pub content_mode: ContentMode,
    pub title: String,
    pub body: String,
    pub requested_at: DateTime<FixedOffset>,
    pub deliver_at: DateTime<FixedOffset>,
    pub quiet_hours_applied: bool,
}

pub fn plan_notification(
    request: NotificationRequest,
    now: DateTime<FixedOffset>,
    quiet_hours: Option<QuietHours>,
) -> Result<NotificationPlan, NotificationError> {
    validate_identifier(&request.idempotency_key)?;
    validate_text(&request.title, 80)?;
    validate_text(&request.body, 240)?;
    if matches!(
        request.privacy,
        PrivacyClass::WorkPrivate | PrivacyClass::Sensitive
    ) && request.content_mode != ContentMode::Generic
    {
        return Err(NotificationError::GenericContentRequired);
    }
    if request.deliver_at <= now || request.deliver_at > now + Duration::days(365) {
        return Err(NotificationError::InvalidDeliveryTime);
    }
    let requested_at = request.deliver_at;
    let (deliver_at, quiet_hours_applied) = match quiet_hours {
        Some(hours) if request.kind != NotificationKind::UrgentAlert => {
            validate_quiet_hours(hours)?;
            apply_quiet_hours(requested_at, hours)
        }
        Some(hours) => {
            validate_quiet_hours(hours)?;
            (requested_at, false)
        }
        None => (requested_at, false),
    };
    Ok(NotificationPlan {
        idempotency_key: request.idempotency_key,
        kind: request.kind,
        privacy: request.privacy,
        content_mode: request.content_mode,
        title: request.title,
        body: request.body,
        requested_at,
        deliver_at,
        quiet_hours_applied,
    })
}

fn apply_quiet_hours(
    delivery: DateTime<FixedOffset>,
    hours: QuietHours,
) -> (DateTime<FixedOffset>, bool) {
    let minute = (delivery.hour() * 60 + delivery.minute()) as u16;
    let quiet = if hours.start_minute < hours.end_minute {
        minute >= hours.start_minute && minute < hours.end_minute
    } else {
        minute >= hours.start_minute || minute < hours.end_minute
    };
    if !quiet {
        return (delivery, false);
    }
    let minutes_until_end = if minute < hours.end_minute {
        hours.end_minute - minute
    } else {
        1440 - minute + hours.end_minute
    };
    let shifted = delivery + Duration::minutes(i64::from(minutes_until_end))
        - Duration::seconds(i64::from(delivery.second()))
        - Duration::nanoseconds(i64::from(delivery.nanosecond()));
    (shifted, true)
}

fn validate_identifier(value: &str) -> Result<(), NotificationError> {
    if !(16..=100).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(NotificationError::InvalidIdentifier);
    }
    Ok(())
}

fn validate_text(value: &str, maximum: usize) -> Result<(), NotificationError> {
    if value.is_empty()
        || value.chars().count() > maximum
        || value.chars().any(|character| character.is_control())
    {
        return Err(NotificationError::InvalidContent);
    }
    Ok(())
}

fn validate_quiet_hours(hours: QuietHours) -> Result<(), NotificationError> {
    if hours.start_minute >= 1440
        || hours.end_minute >= 1440
        || hours.start_minute == hours.end_minute
    {
        return Err(NotificationError::InvalidQuietHours);
    }
    Ok(())
}

pub trait NotificationSink {
    fn record(&mut self, plan: NotificationPlan) -> Result<(), NotificationError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleReconciliation {
    pub schedule: Vec<NotificationPlan>,
    pub retained: Vec<NotificationPlan>,
    pub cancel: Vec<String>,
}

pub fn reconcile_schedule(
    desired: Vec<NotificationPlan>,
    installed_keys: &[String],
) -> Result<ScheduleReconciliation, NotificationError> {
    reconcile_schedule_with_satisfied(desired, installed_keys, &[])
}

pub fn reconcile_schedule_with_satisfied(
    desired: Vec<NotificationPlan>,
    installed_keys: &[String],
    satisfied_keys: &[String],
) -> Result<ScheduleReconciliation, NotificationError> {
    let mut desired_keys = std::collections::BTreeSet::new();
    for plan in &desired {
        validate_identifier(&plan.idempotency_key)?;
        if !desired_keys.insert(plan.idempotency_key.clone()) {
            return Err(NotificationError::DuplicateIdentifier);
        }
    }
    let installed = installed_keys
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let satisfied = satisfied_keys
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let (retained, schedule) = desired.into_iter().partition(|plan| {
        installed.contains(&plan.idempotency_key) || satisfied.contains(&plan.idempotency_key)
    });
    let cancel = installed
        .difference(&desired_keys)
        .cloned()
        .collect::<Vec<_>>();
    Ok(ScheduleReconciliation {
        schedule,
        retained,
        cancel,
    })
}

#[derive(Default)]
pub struct RecordingSink {
    plans: Vec<NotificationPlan>,
}

impl RecordingSink {
    pub fn plans(&self) -> &[NotificationPlan] {
        &self.plans
    }
}

impl NotificationSink for RecordingSink {
    fn record(&mut self, plan: NotificationPlan) -> Result<(), NotificationError> {
        if self
            .plans
            .iter()
            .any(|existing| existing.idempotency_key == plan.idempotency_key)
        {
            return Err(NotificationError::DuplicateIdentifier);
        }
        self.plans.push(plan);
        Ok(())
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NotificationError {
    #[error("notification identifier is invalid")]
    InvalidIdentifier,
    #[error("notification content is invalid")]
    InvalidContent,
    #[error("sensitive notifications require generic content")]
    GenericContentRequired,
    #[error("notification delivery time is invalid")]
    InvalidDeliveryTime,
    #[error("quiet hours are invalid")]
    InvalidQuietHours,
    #[error("notification identifier was already recorded")]
    DuplicateIdentifier,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn base(kind: NotificationKind, hour: u32) -> (NotificationRequest, DateTime<FixedOffset>) {
        let zone = FixedOffset::east_opt(3600).unwrap();
        let now = zone.with_ymd_and_hms(2026, 8, 25, 12, 0, 0).unwrap();
        (
            NotificationRequest {
                idempotency_key: "notification-key-0001".into(),
                kind,
                privacy: PrivacyClass::Private,
                content_mode: ContentMode::Full,
                title: "School reminder".into(),
                body: "Return the permission form tomorrow.".into(),
                deliver_at: zone.with_ymd_and_hms(2026, 8, 25, hour, 30, 15).unwrap(),
            },
            now,
        )
    }

    #[test]
    fn overnight_quiet_hours_shift_routine_delivery_to_the_boundary() {
        let (request, now) = base(NotificationKind::Reminder, 23);
        let plan = plan_notification(
            request,
            now,
            Some(QuietHours {
                start_minute: 22 * 60,
                end_minute: 7 * 60,
            }),
        )
        .unwrap();
        assert_eq!(plan.deliver_at.hour(), 7);
        assert_eq!(plan.deliver_at.minute(), 0);
        assert_eq!(plan.deliver_at.second(), 0);
        assert!(plan.quiet_hours_applied);
    }

    #[test]
    fn urgent_alerts_validate_but_bypass_quiet_hours() {
        let (request, now) = base(NotificationKind::UrgentAlert, 23);
        let requested = request.deliver_at;
        let plan = plan_notification(
            request,
            now,
            Some(QuietHours {
                start_minute: 22 * 60,
                end_minute: 7 * 60,
            }),
        )
        .unwrap();
        assert_eq!(plan.deliver_at, requested);
        assert!(!plan.quiet_hours_applied);
    }

    #[test]
    fn sensitive_content_must_be_generic_and_bounded() {
        let (mut request, now) = base(NotificationKind::Reminder, 13);
        request.privacy = PrivacyClass::Sensitive;
        assert_eq!(
            plan_notification(request.clone(), now, None),
            Err(NotificationError::GenericContentRequired)
        );
        request.content_mode = ContentMode::Generic;
        request.body = "Appointment tomorrow.".into();
        assert!(plan_notification(request, now, None).is_ok());
    }

    #[test]
    fn invalid_times_and_content_fail_closed() {
        let (mut request, now) = base(NotificationKind::MorningSummary, 13);
        request.body = "unsafe\ncontent".into();
        assert_eq!(
            plan_notification(request.clone(), now, None),
            Err(NotificationError::InvalidContent)
        );
        request.body = "Summary".into();
        request.deliver_at = now;
        assert_eq!(
            plan_notification(request, now, None),
            Err(NotificationError::InvalidDeliveryTime)
        );
    }

    #[test]
    fn reconciliation_is_idempotent_and_revokes_stale_requests() {
        let (first, now) = base(NotificationKind::Reminder, 14);
        let mut second = first.clone();
        second.idempotency_key = "notification-key-0002".into();
        second.deliver_at += Duration::hours(1);
        let desired = vec![
            plan_notification(first, now, None).unwrap(),
            plan_notification(second, now, None).unwrap(),
        ];
        let result = reconcile_schedule(
            desired,
            &[
                "notification-key-0001".into(),
                "notification-stale-0001".into(),
            ],
        )
        .unwrap();
        assert_eq!(result.schedule.len(), 1);
        assert_eq!(result.schedule[0].idempotency_key, "notification-key-0002");
        assert_eq!(result.retained.len(), 1);
        assert_eq!(result.retained[0].idempotency_key, "notification-key-0001");
        assert_eq!(result.cancel, ["notification-stale-0001"]);
    }

    #[test]
    fn reconciliation_never_recreates_a_satisfied_past_delivery() {
        let (request, now) = base(NotificationKind::UrgentAlert, 14);
        let plan = plan_notification(request, now, None).unwrap();
        let key = plan.idempotency_key.clone();
        let result = reconcile_schedule_with_satisfied(vec![plan], &[], &[key]).unwrap();
        assert!(result.schedule.is_empty());
        assert_eq!(result.retained.len(), 1);
        assert!(result.cancel.is_empty());
    }

    #[test]
    fn recording_sink_rejects_duplicate_delivery_keys() {
        let (request, now) = base(NotificationKind::EveningSummary, 20);
        let plan = plan_notification(request, now, None).unwrap();
        let mut sink = RecordingSink::default();
        sink.record(plan.clone()).unwrap();
        assert_eq!(
            sink.record(plan),
            Err(NotificationError::DuplicateIdentifier)
        );
        assert_eq!(sink.plans().len(), 1);
    }
}
