use ai::{resolve_temporal_expression, Classification, Extraction};
use assistant_core::AutomationPolicy;
use serde::Serialize;

const CLASSIFICATION_THRESHOLD: f32 = 0.80;
const CANDIDATE_THRESHOLD: f32 = 0.90;
const AUTOMATION_CONFIDENCE_THRESHOLD: f32 = 0.95;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationAction {
    TaskCreate,
    CalendarCreate,
    CalendarUpdate,
    Correspondence,
    Notification,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionSensitivity {
    Routine,
    Important,
    Sensitive,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutomationContext {
    pub action: AutomationAction,
    pub sensitivity: ActionSensitivity,
    pub confidence: f32,
    pub source_verified: bool,
    pub fact_confirmed: bool,
    pub reversible: bool,
    pub user_confirmed: bool,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationDisposition {
    Blocked,
    Review,
    ConfirmationRequired,
    Eligible,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationReason {
    InvalidConfidence,
    UnverifiedSource,
    LowConfidence,
    UnconfirmedCalendarFact,
    IrreversibleAction,
    CorrespondenceAlwaysConfirmed,
    SensitiveAction,
    ImportantAction,
    ConservativePolicy,
    PolicyAllowsRoutineAction,
    UserConfirmed,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutomationDecision {
    pub disposition: AutomationDisposition,
    pub reason: AutomationReason,
}

/// Returns an inert policy decision. This function performs no I/O and cannot execute an action.
pub fn decide_automation(
    policy: AutomationPolicy,
    context: AutomationContext,
) -> AutomationDecision {
    let decision = |disposition, reason| AutomationDecision {
        disposition,
        reason,
    };
    if !context.confidence.is_finite() || !(0.0..=1.0).contains(&context.confidence) {
        return decision(
            AutomationDisposition::Blocked,
            AutomationReason::InvalidConfidence,
        );
    }
    if !context.source_verified {
        return decision(
            AutomationDisposition::Blocked,
            AutomationReason::UnverifiedSource,
        );
    }
    if context.user_confirmed {
        return decision(
            AutomationDisposition::Eligible,
            AutomationReason::UserConfirmed,
        );
    }
    if context.action == AutomationAction::Correspondence {
        return decision(
            AutomationDisposition::ConfirmationRequired,
            AutomationReason::CorrespondenceAlwaysConfirmed,
        );
    }
    if context.sensitivity == ActionSensitivity::Sensitive {
        return decision(
            AutomationDisposition::ConfirmationRequired,
            AutomationReason::SensitiveAction,
        );
    }
    if context.confidence < AUTOMATION_CONFIDENCE_THRESHOLD {
        return decision(
            AutomationDisposition::Review,
            AutomationReason::LowConfidence,
        );
    }
    if matches!(
        context.action,
        AutomationAction::CalendarCreate | AutomationAction::CalendarUpdate
    ) && !context.fact_confirmed
    {
        return decision(
            AutomationDisposition::Review,
            AutomationReason::UnconfirmedCalendarFact,
        );
    }
    if !context.reversible {
        return decision(
            AutomationDisposition::ConfirmationRequired,
            AutomationReason::IrreversibleAction,
        );
    }
    if context.sensitivity == ActionSensitivity::Important {
        return decision(
            AutomationDisposition::ConfirmationRequired,
            AutomationReason::ImportantAction,
        );
    }
    match policy {
        AutomationPolicy::Conservative => decision(
            AutomationDisposition::ConfirmationRequired,
            AutomationReason::ConservativePolicy,
        ),
        AutomationPolicy::Balanced => {
            if matches!(
                context.action,
                AutomationAction::TaskCreate
                    | AutomationAction::CalendarCreate
                    | AutomationAction::Notification
            ) {
                decision(
                    AutomationDisposition::Eligible,
                    AutomationReason::PolicyAllowsRoutineAction,
                )
            } else {
                decision(
                    AutomationDisposition::ConfirmationRequired,
                    AutomationReason::ImportantAction,
                )
            }
        }
        AutomationPolicy::Assistant => decision(
            AutomationDisposition::Eligible,
            AutomationReason::PolicyAllowsRoutineAction,
        ),
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PlanDisposition {
    NoAction,
    Suggestions,
    Review,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Proposal {
    Task { candidate_index: usize },
    Appointment { candidate_index: usize },
    WaitingFor { candidate_index: usize },
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RulePlan {
    pub disposition: PlanDisposition,
    pub proposals: Vec<Proposal>,
    pub review_reasons: Vec<ReviewReason>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReviewReason {
    LowClassificationConfidence,
    LowCandidateConfidence,
    ConflictingNoActionClassification,
    CriticalUrgency,
    AmbiguousTemporalExpression,
    InvalidTemporalOrder,
    MissingAppointmentStart,
}

pub fn evaluate(extraction: &Extraction) -> RulePlan {
    evaluate_internal(extraction, None)
}

pub fn evaluate_with_reference(extraction: &Extraction, reference_at: &str) -> RulePlan {
    evaluate_internal(extraction, Some(reference_at))
}

fn evaluate_internal(extraction: &Extraction, reference_at: Option<&str>) -> RulePlan {
    let mut proposals = Vec::new();
    let mut review_reasons = Vec::new();

    if extraction.classification_confidence < CLASSIFICATION_THRESHOLD {
        review_reasons.push(ReviewReason::LowClassificationConfidence);
    }
    if extraction.urgency == ai::Urgency::Critical {
        review_reasons.push(ReviewReason::CriticalUrgency);
    }

    for (candidate_index, task) in extraction.tasks.iter().enumerate() {
        validate_temporal(task.due_at.as_deref(), reference_at, &mut review_reasons);
        if task.confidence >= CANDIDATE_THRESHOLD {
            proposals.push(Proposal::Task { candidate_index });
        } else {
            add_reason_once(&mut review_reasons, ReviewReason::LowCandidateConfidence);
        }
    }
    for (candidate_index, appointment) in extraction.appointments.iter().enumerate() {
        if appointment.start_at.is_none() {
            add_reason_once(&mut review_reasons, ReviewReason::MissingAppointmentStart);
        }
        let start = resolve_optional(appointment.start_at.as_deref(), reference_at);
        let end = resolve_optional(appointment.end_at.as_deref(), reference_at);
        if matches!(start, Some(Err(_))) || matches!(end, Some(Err(_))) {
            add_reason_once(
                &mut review_reasons,
                ReviewReason::AmbiguousTemporalExpression,
            );
        }
        if let (Some(Ok(start)), Some(Ok(end))) = (&start, &end) {
            if (&end.local_date, &end.local_time) < (&start.local_date, &start.local_time) {
                add_reason_once(&mut review_reasons, ReviewReason::InvalidTemporalOrder);
            }
        }
        if appointment.confidence >= CANDIDATE_THRESHOLD {
            proposals.push(Proposal::Appointment { candidate_index });
        } else {
            add_reason_once(&mut review_reasons, ReviewReason::LowCandidateConfidence);
        }
    }
    for (candidate_index, waiting) in extraction.waiting_for.iter().enumerate() {
        validate_temporal(
            waiting.follow_up_at.as_deref(),
            reference_at,
            &mut review_reasons,
        );
        if waiting.confidence >= CANDIDATE_THRESHOLD {
            proposals.push(Proposal::WaitingFor { candidate_index });
        } else {
            add_reason_once(&mut review_reasons, ReviewReason::LowCandidateConfidence);
        }
    }

    if extraction.classification == Classification::NoActionNeeded && !proposals.is_empty() {
        review_reasons.push(ReviewReason::ConflictingNoActionClassification);
    }

    let disposition = if !review_reasons.is_empty() {
        PlanDisposition::Review
    } else if proposals.is_empty() {
        PlanDisposition::NoAction
    } else {
        PlanDisposition::Suggestions
    };
    RulePlan {
        disposition,
        proposals,
        review_reasons,
    }
}

fn validate_temporal(
    expression: Option<&str>,
    reference_at: Option<&str>,
    reasons: &mut Vec<ReviewReason>,
) {
    if matches!(resolve_optional(expression, reference_at), Some(Err(_))) {
        add_reason_once(reasons, ReviewReason::AmbiguousTemporalExpression);
    }
}

fn resolve_optional(
    expression: Option<&str>,
    reference_at: Option<&str>,
) -> Option<Result<ai::ResolvedTemporal, ai::TemporalError>> {
    expression.map(|expression| {
        reference_at
            .ok_or(ai::TemporalError::InvalidReference)
            .and_then(|reference| resolve_temporal_expression(expression, reference))
    })
}

fn add_reason_once(reasons: &mut Vec<ReviewReason>, reason: ReviewReason) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::{parse_and_validate_extraction, Urgency};

    fn safe_context(action: AutomationAction) -> AutomationContext {
        AutomationContext {
            action,
            sensitivity: ActionSensitivity::Routine,
            confidence: 0.98,
            source_verified: true,
            fact_confirmed: true,
            reversible: true,
            user_confirmed: false,
        }
    }

    fn extraction(
        classification: &str,
        classification_confidence: f32,
        task_confidence: f32,
    ) -> Extraction {
        let source = "Please return the form.";
        let json = format!(
            r#"{{
              "schemaVersion": 1,
              "classification": "{classification}",
              "classificationConfidence": {classification_confidence},
              "urgency": "normal",
              "summary": "A form must be returned.",
              "tasks": [{{
                "title": "Return the form",
                "dueAt": null,
                "confidence": {task_confidence},
                "evidence": {{"byteStart": 7, "byteEnd": 22, "quote": "return the form"}}
              }}],
              "appointments": [],
              "waitingFor": []
            }}"#
        );
        parse_and_validate_extraction(source, json.as_bytes()).unwrap()
    }

    #[test]
    fn high_confidence_output_creates_only_an_inert_suggestion() {
        let plan = evaluate(&extraction("task", 0.95, 0.96));
        assert_eq!(plan.disposition, PlanDisposition::Suggestions);
        assert_eq!(plan.proposals, vec![Proposal::Task { candidate_index: 0 }]);
    }

    #[test]
    fn low_confidence_output_is_forced_to_review() {
        let plan = evaluate(&extraction("task", 0.60, 0.70));
        assert_eq!(plan.disposition, PlanDisposition::Review);
        assert!(plan
            .review_reasons
            .contains(&ReviewReason::LowClassificationConfidence));
        assert!(plan
            .review_reasons
            .contains(&ReviewReason::LowCandidateConfidence));
    }

    #[test]
    fn contradictory_no_action_output_is_forced_to_review() {
        let plan = evaluate(&extraction("no_action_needed", 0.95, 0.96));
        assert_eq!(plan.disposition, PlanDisposition::Review);
        assert!(plan
            .review_reasons
            .contains(&ReviewReason::ConflictingNoActionClassification));
    }

    #[test]
    fn critical_urgency_is_never_automatically_accepted() {
        let mut extraction = extraction("task", 0.95, 0.96);
        extraction.urgency = Urgency::Critical;
        assert_eq!(evaluate(&extraction).disposition, PlanDisposition::Review);
    }

    #[test]
    fn ambiguous_model_date_is_forced_to_review() {
        let mut extraction = extraction("task", 0.95, 0.96);
        extraction.tasks[0].due_at = Some("sometime next week".into());
        let plan = evaluate_with_reference(&extraction, "2026-08-13T09:30:00+01:00");
        assert_eq!(plan.disposition, PlanDisposition::Review);
        assert!(plan
            .review_reasons
            .contains(&ReviewReason::AmbiguousTemporalExpression));
    }

    #[test]
    fn deterministic_relative_date_can_remain_a_suggestion() {
        let mut extraction = extraction("task", 0.95, 0.96);
        extraction.tasks[0].due_at = Some("tomorrow".into());
        let plan = evaluate_with_reference(&extraction, "2026-08-13T09:30:00+01:00");
        assert_eq!(plan.disposition, PlanDisposition::Suggestions);
    }

    #[test]
    fn automation_fails_closed_for_invalid_or_unverified_input() {
        let mut context = safe_context(AutomationAction::TaskCreate);
        context.confidence = f32::NAN;
        assert_eq!(
            decide_automation(AutomationPolicy::Assistant, context).disposition,
            AutomationDisposition::Blocked
        );
        context.confidence = 0.99;
        context.source_verified = false;
        assert_eq!(
            decide_automation(AutomationPolicy::Assistant, context).reason,
            AutomationReason::UnverifiedSource
        );
    }

    #[test]
    fn conservative_policy_requires_confirmation_for_routine_actions() {
        let decision = decide_automation(
            AutomationPolicy::Conservative,
            safe_context(AutomationAction::TaskCreate),
        );
        assert_eq!(
            decision.disposition,
            AutomationDisposition::ConfirmationRequired
        );
        assert_eq!(decision.reason, AutomationReason::ConservativePolicy);
    }

    #[test]
    fn balanced_allows_only_high_confidence_confirmed_routine_creation() {
        let task = decide_automation(
            AutomationPolicy::Balanced,
            safe_context(AutomationAction::TaskCreate),
        );
        assert_eq!(task.disposition, AutomationDisposition::Eligible);
        let mut calendar = safe_context(AutomationAction::CalendarCreate);
        calendar.fact_confirmed = false;
        assert_eq!(
            decide_automation(AutomationPolicy::Balanced, calendar).reason,
            AutomationReason::UnconfirmedCalendarFact
        );
        let update = decide_automation(
            AutomationPolicy::Balanced,
            safe_context(AutomationAction::CalendarUpdate),
        );
        assert_eq!(
            update.disposition,
            AutomationDisposition::ConfirmationRequired
        );
    }

    #[test]
    fn correspondence_and_sensitive_actions_always_require_confirmation() {
        for policy in [
            AutomationPolicy::Conservative,
            AutomationPolicy::Balanced,
            AutomationPolicy::Assistant,
        ] {
            assert_eq!(
                decide_automation(policy, safe_context(AutomationAction::Correspondence)).reason,
                AutomationReason::CorrespondenceAlwaysConfirmed
            );
            let mut sensitive = safe_context(AutomationAction::Notification);
            sensitive.sensitivity = ActionSensitivity::Sensitive;
            assert_eq!(
                decide_automation(policy, sensitive).disposition,
                AutomationDisposition::ConfirmationRequired
            );
        }
    }

    #[test]
    fn explicit_confirmation_still_requires_a_verified_source() {
        let mut context = safe_context(AutomationAction::CalendarUpdate);
        context.user_confirmed = true;
        assert_eq!(
            decide_automation(AutomationPolicy::Conservative, context).disposition,
            AutomationDisposition::Eligible
        );
        context.source_verified = false;
        assert_eq!(
            decide_automation(AutomationPolicy::Conservative, context).disposition,
            AutomationDisposition::Blocked
        );
    }
}
