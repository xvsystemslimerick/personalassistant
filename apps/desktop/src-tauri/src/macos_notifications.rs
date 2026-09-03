use block2::{DynBlock, RcBlock};
use objc2::{
    define_class, msg_send, rc::Allocated, runtime::ProtocolObject, MainThreadMarker,
    MainThreadOnly,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationSound,
    UNTimeIntervalNotificationTrigger, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::ptr::NonNull;
use tokio::sync::oneshot;

#[derive(Debug)]
pub struct ScheduleOutcome {
    pub delivery_key: String,
    pub event_type: &'static str,
    pub kind: notifications::NotificationKind,
    pub deliver_at: String,
    pub reason_code: &'static str,
}

#[derive(Debug)]
pub struct ReconcileOutcome {
    pub scheduled: Vec<ScheduleOutcome>,
    pub cancelled: Vec<String>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct PersonalAssistantNotificationDelegate;

    impl PersonalAssistantNotificationDelegate {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> objc2::rc::Retained<Self> {
            let this = this.set_ivars(());
            unsafe { msg_send![super(this), init] }
        }
    }

    unsafe impl NSObjectProtocol for PersonalAssistantNotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for PersonalAssistantNotificationDelegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }
    }
);

pub fn install_foreground_delegate() {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let main_thread =
        MainThreadMarker::new().expect("notification delegate must install on main thread");
    let delegate: objc2::rc::Retained<PersonalAssistantNotificationDelegate> = unsafe {
        msg_send![
            PersonalAssistantNotificationDelegate::alloc(main_thread),
            init
        ]
    };
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    let _ = objc2::rc::Retained::into_raw(delegate);
}

fn permission_status_receiver() -> oneshot::Receiver<UNAuthorizationStatus> {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (sender, receiver) = oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    let completion = RcBlock::new(
        move |settings: NonNull<objc2_user_notifications::UNNotificationSettings>| {
            let status = unsafe { settings.as_ref() }.authorizationStatus();
            if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
                let _ = sender.send(status);
            }
        },
    );
    center.getNotificationSettingsWithCompletionHandler(&completion);
    receiver
}

pub async fn permission_status() -> Result<&'static str, String> {
    let receiver = permission_status_receiver();
    let status = tokio::time::timeout(std::time::Duration::from_secs(8), receiver)
        .await
        .map_err(|_| "notification_status_timed_out".to_owned())?
        .map_err(|_| "notification_status_unavailable".to_owned())?;
    Ok(match status {
        UNAuthorizationStatus::Authorized
        | UNAuthorizationStatus::Provisional
        | UNAuthorizationStatus::Ephemeral => "granted",
        UNAuthorizationStatus::Denied => "denied",
        _ => "notDetermined",
    })
}

fn permission_request_receiver() -> oneshot::Receiver<bool> {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (sender, receiver) = oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    let completion = RcBlock::new(move |granted: objc2::runtime::Bool, error: *mut NSError| {
        if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
            let _ = sender.send(error.is_null() && granted.as_bool());
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &completion,
    );
    receiver
}

pub async fn request_permission() -> Result<&'static str, String> {
    let receiver = permission_request_receiver();
    Ok(
        if tokio::time::timeout(std::time::Duration::from_secs(30), receiver)
            .await
            .map_err(|_| "notification_permission_timed_out".to_owned())?
            .map_err(|_| "notification_permission_unavailable".to_owned())?
        {
            "granted"
        } else {
            "denied"
        },
    )
}

fn delivery_receiver(event_key: &str) -> oneshot::Receiver<bool> {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str("Personal Assistant"));
    content.setBody(&NSString::from_str(
        "Notifications are working. No email content is included.",
    ));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(event_key),
        &content,
        None,
    );
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (sender, receiver) = oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    let completion = RcBlock::new(move |error: *mut NSError| {
        if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
            let _ = sender.send(error.is_null());
        }
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&completion));
    receiver
}

fn delivered_receiver(event_key: String) -> oneshot::Receiver<bool> {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (sender, receiver) = oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    let completion = RcBlock::new(move |notifications: NonNull<NSArray<UNNotification>>| {
        let delivered = unsafe { notifications.as_ref() }
            .iter()
            .any(|notification| notification.request().identifier().to_string() == event_key);
        if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
            let _ = sender.send(delivered);
        }
    });
    center.getDeliveredNotificationsWithCompletionHandler(&completion);
    receiver
}

pub async fn send_generic_test(event_key: &str) -> Result<&'static str, String> {
    let receiver = delivery_receiver(event_key);
    if !tokio::time::timeout(std::time::Duration::from_secs(8), receiver)
        .await
        .map_err(|_| "notification_submission_timed_out".to_owned())?
        .map_err(|_| "notification_delivery_unavailable".to_owned())?
    {
        return Err("notification_delivery_rejected".to_owned());
    }
    tokio::time::sleep(std::time::Duration::from_millis(750)).await;
    let delivered = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        delivered_receiver(event_key.to_owned()),
    )
    .await
    .map_err(|_| "notification_receipt_timed_out".to_owned())?
    .map_err(|_| "notification_receipt_unavailable".to_owned())?;
    Ok(if delivered { "delivered" } else { "accepted" })
}

fn pending_keys_receiver() -> oneshot::Receiver<Vec<String>> {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (sender, receiver) = oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    let completion = RcBlock::new(move |requests: NonNull<NSArray<UNNotificationRequest>>| {
        let keys = unsafe { requests.as_ref() }
            .iter()
            .map(|request| request.identifier().to_string())
            .collect();
        if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
            let _ = sender.send(keys);
        }
    });
    center.getPendingNotificationRequestsWithCompletionHandler(&completion);
    receiver
}

fn scheduled_delivery_receiver(plan: &notifications::NotificationPlan) -> oneshot::Receiver<bool> {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(&plan.title));
    content.setBody(&NSString::from_str(&plan.body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    let seconds = (plan.deliver_at - chrono::Utc::now().fixed_offset())
        .num_milliseconds()
        .max(1_000) as f64
        / 1_000.0;
    let trigger =
        UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(seconds, false);
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(&plan.idempotency_key),
        &content,
        Some(&trigger),
    );
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let (sender, receiver) = oneshot::channel();
    let sender = std::sync::Mutex::new(Some(sender));
    let completion = RcBlock::new(move |error: *mut NSError| {
        if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
            let _ = sender.send(error.is_null());
        }
    });
    center.addNotificationRequest_withCompletionHandler(&request, Some(&completion));
    receiver
}

fn cancel_pending(keys: &[String]) {
    let identifiers = keys
        .iter()
        .map(|key| NSString::from_str(key))
        .collect::<Vec<_>>();
    let identifiers = NSArray::from_retained_slice(&identifiers);
    UNUserNotificationCenter::currentNotificationCenter()
        .removePendingNotificationRequestsWithIdentifiers(&identifiers);
}

fn is_automatic_delivery_key(key: &str) -> bool {
    !key.starts_with("scheduled-")
}

pub async fn automatic_pending_count() -> Result<usize, String> {
    let pending = tokio::time::timeout(std::time::Duration::from_secs(8), pending_keys_receiver())
        .await
        .map_err(|_| "notification_pending_query_timed_out".to_owned())?
        .map_err(|_| "notification_pending_query_unavailable".to_owned())?;
    Ok(pending
        .iter()
        .filter(|key| is_automatic_delivery_key(key))
        .count())
}

pub async fn reconcile_schedule(
    desired: Vec<notifications::NotificationPlan>,
    satisfied_keys: &[String],
) -> Result<ReconcileOutcome, String> {
    let installed =
        tokio::time::timeout(std::time::Duration::from_secs(8), pending_keys_receiver())
            .await
            .map_err(|_| "notification_pending_query_timed_out".to_owned())?
            .map_err(|_| "notification_pending_query_unavailable".to_owned())?
            .into_iter()
            .filter(|key| is_automatic_delivery_key(key))
            .collect::<Vec<_>>();
    let reconciliation =
        notifications::reconcile_schedule_with_satisfied(desired, &installed, satisfied_keys)
            .map_err(|_| "notification_reconciliation_invalid".to_owned())?;
    let cancelled = reconciliation.cancel;
    if !cancelled.is_empty() {
        cancel_pending(&cancelled);
    }
    let mut scheduled = reconciliation
        .retained
        .into_iter()
        .map(|plan| ScheduleOutcome {
            delivery_key: plan.idempotency_key,
            event_type: "scheduled",
            kind: plan.kind,
            deliver_at: plan.deliver_at.to_rfc3339(),
            reason_code: "desired_plan",
        })
        .collect::<Vec<_>>();
    for plan in reconciliation.schedule {
        let accepted = tokio::time::timeout(
            std::time::Duration::from_secs(8),
            scheduled_delivery_receiver(&plan),
        )
        .await
        .map_err(|_| "notification_schedule_timed_out".to_owned())?
        .map_err(|_| "notification_schedule_unavailable".to_owned())?;
        if !accepted {
            return Err("notification_schedule_rejected".to_owned());
        }
        scheduled.push(ScheduleOutcome {
            delivery_key: plan.idempotency_key,
            event_type: "scheduled",
            kind: plan.kind,
            deliver_at: plan.deliver_at.to_rfc3339(),
            reason_code: "desired_plan",
        });
    }
    Ok(ReconcileOutcome {
        scheduled,
        cancelled,
    })
}

pub async fn schedule_generic_test(
    delivery_key: &str,
) -> Result<notifications::NotificationPlan, String> {
    let now = chrono::Utc::now().fixed_offset();
    let plan = notifications::plan_notification(
        notifications::NotificationRequest {
            idempotency_key: delivery_key.to_owned(),
            kind: notifications::NotificationKind::Reminder,
            privacy: notifications::PrivacyClass::Private,
            content_mode: notifications::ContentMode::Generic,
            title: "Personal Assistant".into(),
            body: "Scheduled notifications are working. No email content is included.".into(),
            deliver_at: now + chrono::Duration::seconds(60),
        },
        now,
        None,
    )
    .map_err(|_| "scheduled_test_invalid".to_owned())?;
    let accepted = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        scheduled_delivery_receiver(&plan),
    )
    .await
    .map_err(|_| "scheduled_test_timed_out".to_owned())?
    .map_err(|_| "scheduled_test_unavailable".to_owned())?;
    if !accepted {
        return Err("scheduled_test_rejected".to_owned());
    }
    let pending = tokio::time::timeout(std::time::Duration::from_secs(8), pending_keys_receiver())
        .await
        .map_err(|_| "scheduled_test_receipt_timed_out".to_owned())?
        .map_err(|_| "scheduled_test_receipt_unavailable".to_owned())?;
    if !pending.iter().any(|key| key == delivery_key) {
        return Err("scheduled_test_receipt_missing".to_owned());
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::is_automatic_delivery_key;

    #[test]
    fn pending_status_excludes_isolated_scheduled_diagnostics() {
        assert!(!is_automatic_delivery_key("scheduled-diagnostic-id"));
        assert!(is_automatic_delivery_key("notification-reminder-id"));
    }
}
