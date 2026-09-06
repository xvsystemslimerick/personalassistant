import { invoke } from "@tauri-apps/api/core";
import type { ActionProposal, AiCapabilities, AiEvaluationReport, AiRuntimeHealth, AutomationAuditEntry, CalendarUpdateCandidate, CorrespondenceExecutionHistory, FamilyDisplayPairingChallenge, FamilyDisplayRecord, FamilyDisplayServiceStatus, HomeDashboard, LocalItem, LocalItemEvent, LocalItemProvenance, MessageAnalysisResult, MicrosoftAccount, MicrosoftConnectionStatus, MicrosoftSyncResult, NotificationPreview, ReplyDraftMetadata, ReviewDecision, ReviewItem, Settings } from "./types";

export interface SettingsApi {
  load(): Promise<Settings>;
  save(settings: Settings): Promise<Settings>;
}

export const tauriSettingsApi: SettingsApi = {
  load: () => invoke<Settings>("get_settings"),
  save: (settings) => invoke<Settings>("save_settings", { settings })
};

export const microsoftApi = {
  status: () => invoke<MicrosoftConnectionStatus>("microsoft_status"),
  connect: () => invoke<MicrosoftAccount>("connect_microsoft"),
  sync: (accountId: string) => invoke<MicrosoftSyncResult>("sync_microsoft", { accountId }),
  analyzeMessage: (accountId: string, providerId: string) => invoke<MessageAnalysisResult>("analyze_microsoft_message", { accountId, providerId }),
  cancelSync: (accountId: string) => invoke<boolean>("cancel_microsoft_sync", { accountId }),
  disconnect: (accountId: string, deleteLocalData: boolean) => invoke<void>("disconnect_microsoft", { accountId, deleteLocalData }),
  deleteLocalData: (accountId: string) => invoke<void>("delete_microsoft_local_data", { accountId }),
  saveReplyDraft: (accountId: string, providerMessageId: string, comment: string) => invoke<ReplyDraftMetadata>("save_reply_draft", { draftKey: crypto.randomUUID(), revisionKey: crypto.randomUUID(), accountId, providerMessageId, comment }),
  verifyReplyDrafts: (accountId: string) => invoke<number>("verify_reply_draft_storage", { accountId }),
  queueReplyDraft: (accountId: string, providerMessageId: string) => invoke<ActionProposal>("queue_reply_draft_proposal", { proposalKey: crypto.randomUUID(), auditEventKey: crypto.randomUUID(), accountId, providerMessageId })
};

export const reviewApi = {
  list: () => invoke<ReviewItem[]>("review_items"),
  history: () => invoke<ReviewDecision[]>("review_history"),
  decide: (accountId: string, providerId: string, decision: "accept" | "ignore") => invoke<ReviewDecision>("decide_review", { accountId, providerId, decision })
};

export const localItemsApi = {
  list: () => invoke<LocalItem[]>("local_items"),
  undo: (id: number) => invoke<LocalItem>("undo_local_item", { id }),
  transition: (id: number, eventKey: string, eventType: "complete" | "snooze" | "reschedule", scheduledAt?: string) => invoke<LocalItem>("transition_local_item", { id, eventKey, eventType, scheduledAt }),
  events: () => invoke<LocalItemEvent[]>("local_item_events"),
  focus: (view: "today" | "work" | "kids" | "personal", dayStart: string, dayEnd: string) => invoke<LocalItem[]>("focused_local_items", { view, dayStart, dayEnd }),
  provenance: (id: number) => invoke<LocalItemProvenance>("local_item_provenance", { id }),
  openSource: (id: number) => invoke<void>("open_local_item_source", { id })
};

export const homeApi = {
  dashboard: (dayStart: string, dayEnd: string) => invoke<HomeDashboard>("home_dashboard", { dayStart, dayEnd })
};

export const actionProposalsApi = {
  list: (now: string) => invoke<ActionProposal[]>("action_proposals", { now }),
  correspondenceHistory: () => invoke<CorrespondenceExecutionHistory[]>("correspondence_execution_history"),
  auditHistory: () => invoke<AutomationAuditEntry[]>("automation_audit_history"),
  decide: (proposalKey: string, eventKey: string, eventType: "confirm" | "cancel") => invoke<ActionProposal>("decide_action_proposal", { proposalKey, eventKey, eventType }),
  executeCalendar: (proposalKey: string, executionKey: string) => invoke<string>("execute_calendar_proposal", { proposalKey, executionKey }),
  calendarUpdateCandidates: (accountId: string) => invoke<CalendarUpdateCandidate[]>("calendar_update_candidates", { accountId }),
  queueCalendarUpdate: (accountId: string, providerEventId: string, proposedStartAt: string, proposedEndAt: string) => invoke<ActionProposal>("queue_calendar_update_proposal", { proposalKey: crypto.randomUUID(), auditEventKey: crypto.randomUUID(), accountId, providerEventId, proposedStartAt, proposedEndAt }),
  executeCalendarUpdate: (proposalKey: string, executionKey: string) => invoke<string>("execute_calendar_update_proposal", { proposalKey, executionKey })
  ,executeCorrespondence: (proposalKey: string, executionKey: string) => invoke<string>("execute_correspondence_proposal", { proposalKey, executionKey })
};

export const notificationPreviewsApi = {
  list: (now: string) => invoke<NotificationPreview[]>("notification_previews", { now }),
  permissionStatus: () => invoke<"granted" | "denied" | "notDetermined">("notification_permission_status"),
  scheduleStatus: () => invoke<number>("notification_schedule_status"),
  requestPermission: () => invoke<"granted" | "denied">("request_notification_permission"),
  sendGenericTest: (eventKey: string) => invoke<"delivered" | "accepted">("send_generic_test_notification", { eventKey }),
  scheduleGenericTest: (eventKey: string) => invoke<string>("schedule_generic_test_notification", { eventKey })
};

export const familyDisplaysApi = {
  list: () => invoke<FamilyDisplayRecord[]>("family_displays"),
  serviceStatus: () => invoke<FamilyDisplayServiceStatus>("family_display_service_status"),
  beginPairing: () => invoke<FamilyDisplayPairingChallenge>("begin_family_display_pairing"),
  enableService: (bindAddress: string) => invoke<FamilyDisplayServiceStatus>("enable_family_display_service", { bindAddress, eventKey: crypto.randomUUID() }),
  disableService: () => invoke<FamilyDisplayServiceStatus>("disable_family_display_service", { eventKey: crypto.randomUUID() }),
  revoke: (displayId: string) => invoke<FamilyDisplayRecord[]>("revoke_family_display", { displayId, eventKey: crypto.randomUUID() }),
};

export const backupApi = {
  create: (destination: string, password: string) => invoke<string>("create_encrypted_backup", { destination, password }),
  verify: (source: string, password: string) => invoke<string>("verify_encrypted_backup", { source, password }),
  prepareRestore: (source: string, password: string) => invoke<string>("prepare_encrypted_restore", { source, password }),
  restartForRestore: () => invoke<void>("restart_for_restore"),
  restoreStatus: () => invoke<string | null>("restore_status")
};

export const aiApi = {
  capabilities: () => invoke<AiCapabilities>("ai_capabilities"),
  download: () => invoke<AiCapabilities>("download_ai_model"),
  remove: () => invoke<AiCapabilities>("remove_ai_model"),
  testRuntime: () => invoke<AiRuntimeHealth>("test_ai_runtime"),
  testEvaluation: () => invoke<AiEvaluationReport>("test_ai_evaluation"),
  testPersistentEvaluation: () => invoke<AiEvaluationReport>("test_persistent_ai_evaluation")
};
