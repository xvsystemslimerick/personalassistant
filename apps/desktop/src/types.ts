export type Theme = "system" | "light" | "dark";
export type AutomationPolicy = "conservative" | "balanced" | "assistant";
export interface Settings {
  householdName: string;
  theme: Theme;
  launchAtLogin: boolean;
  storeCompleteEmailContent: boolean;
  localEmailAnalysisEnabled: boolean;
  automationPolicy: AutomationPolicy;
  notificationDeliveryEnabled: boolean;
  urgentAlertsEnabled: boolean;
  appointmentRemindersEnabled: boolean;
  morningSummaryEnabled: boolean;
  eveningSummaryEnabled: boolean;
  quietHoursStartMinute: number;
  quietHoursEndMinute: number;
}

export interface MicrosoftAccount {
  id: string;
  displayName: string;
  emailAddress: string;
  lastSync: { outcome: string; finishedAt?: string; itemCount: number } | null;
  recentMessages: {
    providerId: string;
    subject?: string;
    occurredAt?: string;
    analyzed: boolean;
    hasReplyDraft: boolean;
  }[];
}

export interface MessageAnalysisResult {
  providerId: string;
  summary: string;
  classification: string;
  urgency: string;
  disposition: "noAction" | "suggestions" | "review";
  suggestionCount: number;
  localProjectionCount: number;
}

export interface ReplyDraftMetadata {
  draftKey: string;
  revisionKey: string;
  accountId: string;
  providerMessageId: string;
  contentSha256: string;
  contentBytes: number;
  createdAt: string;
}

export type ReviewSuggestion =
  | { kind: "task"; title: string; due_at?: string }
  | { kind: "appointment"; title: string; start_at?: string; end_at?: string; confirmed: boolean }
  | { kind: "waiting_for"; description: string; expected_from?: string; follow_up_at?: string };

export interface ReviewItem {
  accountId: string;
  providerId: string;
  subject?: string;
  senderName?: string;
  occurredAt?: string;
  summary: string;
  classification: string;
  classificationConfidence: number;
  urgency: string;
  suggestions: ReviewSuggestion[];
  reviewReasons: string[];
  analyzedAt: string;
}

export interface ReviewDecision {
  accountId: string;
  providerId: string;
  subject?: string;
  decision: "accept" | "ignore";
  decidedAt: string;
}

export interface LocalItem {
  id: number;
  accountId: string;
  providerId: string;
  suggestionIndex: number;
  kind: "task" | "appointment" | "waiting_for";
  title: string;
  dueAt?: string;
  startAt?: string;
  endAt?: string;
  expectedFrom?: string;
  followUpAt?: string;
  category: string;
  status: "active" | "undone";
  lifecycleState: "active" | "completed" | "snoozed";
  scheduledAt?: string;
  createdAt: string;
  undoneAt?: string;
}

export interface LocalItemEvent {
  id: number;
  itemId: number;
  eventType: "complete" | "snooze" | "reschedule";
  previousState: string;
  newState: string;
  previousScheduledAt?: string;
  newScheduledAt?: string;
  createdAt: string;
}

export interface LocalItemProvenance {
  itemId: number;
  provider: string;
  sourceSubject?: string;
  sourceSender?: string;
  sourceOccurredAt?: string;
  sourceAvailable: boolean;
}

export interface ActionProposal {
  id: number;
  proposalKey: string;
  auditEventKey: string;
  actionKind: "task_create" | "calendar_create" | "calendar_update" | "correspondence" | "notification";
  sensitivity: "routine" | "important" | "sensitive";
  policy: "conservative" | "balanced" | "assistant";
  disposition: "review" | "confirmation_required" | "eligible";
  reasonCode: string;
  displayLabel: string;
  targetRef?: string;
  confidence: number;
  expiresAt: string;
  createdAt: string;
  state: "pending" | "confirmed" | "cancelled" | "expired" | "closed";
}

export interface CorrespondenceExecutionHistory {
  displayLabel: string;
  outcome: "succeeded" | "failed" | "unknown";
  reasonCode: string;
  createdAt: string;
}

export interface AutomationAuditEntry {
  policy: "conservative" | "balanced" | "assistant";
  decision: string;
  actionKind: string;
  reasonCode: string;
  createdAt: string;
}

export interface CalendarUpdateCandidate {
  accountId: string;
  providerId: string;
  subject: string;
  startAt: string;
  startTimezone: string;
  endAt: string;
  endTimezone: string;
}

export interface NotificationPreview {
  idempotencyKey: string;
  kind: "reminder" | "urgent_alert" | "morning_summary" | "evening_summary";
  privacy: "PUBLIC_FAMILY" | "PRIVATE" | "WORK_PRIVATE" | "SENSITIVE";
  contentMode: "full" | "generic";
  title: string;
  body: string;
  requestedAt: string;
  deliverAt: string;
  quietHoursApplied: boolean;
}

export interface FamilyDisplayRecord {
  id: string;
  displayName: string;
  revoked: boolean;
  lastSeenAt?: string;
  createdAt: string;
}

export interface FamilyDisplayServiceStatus {
  enabled: boolean;
  running: boolean;
  bindAddress?: string;
  certificateSha256?: string;
}

export interface FamilyDisplayPairingChallenge {
  code: string;
  expiresAtUnix: number;
}

export interface HomeDashboard {
  today: LocalItem[];
  todoCount: number;
  waitingCount: number;
  calendarCount: number;
  reviewCount: number;
  sync: {
    connectedAccounts: number;
    healthyAccounts: number;
    attentionAccounts: number;
    lastSyncAt?: string;
  };
}

export interface MicrosoftConnectionStatus {
  configured: boolean;
  accounts: MicrosoftAccount[];
}

export interface MicrosoftSyncResult {
  inbox: number;
  sent: number;
  calendar: number;
}

export interface AiCapabilities {
  hardware: {
    architecture: string;
    logicalCpuCount: number;
    physicalMemoryBytes: number;
    availableDiskBytes: number;
    appleSilicon: boolean;
    acceleration: "metal" | "cpu";
  };
  recommendation: {
    tier: "compact" | "standard";
    estimatedDownloadBytes: number;
    requiredStorageBytes: number;
    eligible: boolean;
    reason: string;
    artifact: {
      id: string;
      displayName: string;
      tier: "compact" | "standard";
      revision: string;
      filename: string;
      url: string;
      sizeBytes: number;
      sha256: string;
      license: string;
    };
  };
  lifecycle: "notInstalled" | "installed";
}

export interface AiDownloadProgress { downloadedBytes: number; totalBytes: number; }
export interface AiRuntimeHealth { ready: boolean; runtimeVersion: string; acceleration: "metal" | "cpu"; modelId: string; }
export interface AiEvaluationReport {
  passed: number;
  total: number;
  allPassed: boolean;
  cases: {
    id: string;
    passed: boolean;
    observedClassification?: string;
    taskCount?: number;
    appointmentCount?: number;
    waitingCount?: number;
    safetyRejected: boolean;
    failureCode?: string;
  }[];
}
