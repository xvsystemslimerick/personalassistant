import { FormEvent, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { actionProposalsApi, aiApi, familyDisplaysApi, homeApi, localItemsApi, microsoftApi, notificationPreviewsApi, reviewApi, tauriSettingsApi, type SettingsApi } from "./settings-api";
import type { ActionProposal, AiCapabilities, AiDownloadProgress, AiEvaluationReport, AiRuntimeHealth, AutomationAuditEntry, AutomationPolicy, CalendarUpdateCandidate, CorrespondenceExecutionHistory, FamilyDisplayPairingChallenge, FamilyDisplayRecord, FamilyDisplayServiceStatus, HomeDashboard, LocalItem, LocalItemEvent, LocalItemProvenance, MicrosoftConnectionStatus, NotificationPreview, ReviewDecision, ReviewItem, ReviewSuggestion, Settings, Theme } from "./types";
import "./styles.css";

const initial: Settings = { householdName: "", theme: "system", launchAtLogin: false, storeCompleteEmailContent: false, localEmailAnalysisEnabled: false, automationPolicy: "balanced", notificationDeliveryEnabled: false, urgentAlertsEnabled: false, appointmentRemindersEnabled: false, morningSummaryEnabled: false, eveningSummaryEnabled: false, quietHoursStartMinute: 1320, quietHoursEndMinute: 420 };

function FamilyDisplaySetupDetails({ challenge }: { challenge: FamilyDisplayPairingChallenge }) {
  return <aside className="pairing-material" aria-label="Raspberry Pi setup values">
    <strong>Raspberry Pi setup values</strong>
    <dl>
      <div><dt>Host</dt><dd><code>{challenge.hostUrl}</code></dd></div>
      <div><dt>Certificate SHA-256</dt><dd><code>{challenge.certificateSha256}</code></dd></div>
      <div><dt>Public certificate (DER, Base64)</dt><dd><code>{challenge.certificateDerBase64}</code></dd></div>
    </dl>
    <small>Decode the Base64 value to <code>personal-assistant.der</code> on the Pi. This is public trust material; the TLS private key remains in macOS Keychain.</small>
  </aside>;
}

export function App({ api = tauriSettingsApi }: { api?: SettingsApi }) {
  const [settings, setSettings] = useState(initial);
  const [state, setState] = useState<"loading" | "ready" | "saving" | "saved" | "error">("loading");
  const [error, setError] = useState("");
  const [microsoft, setMicrosoft] = useState<MicrosoftConnectionStatus | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [connectionError, setConnectionError] = useState("");
  const [accountAction, setAccountAction] = useState<string | null>(null);
  const [syncMessage, setSyncMessage] = useState("");
  const [analyzingMessage, setAnalyzingMessage] = useState<string | null>(null);
  const [pendingAnalysis, setPendingAnalysis] = useState<{ accountId: string; providerId: string; subject?: string } | null>(null);
  const [pendingReplyDraft, setPendingReplyDraft] = useState<{ accountId: string; providerId: string; subject?: string } | null>(null);
  const [replyDraftText, setReplyDraftText] = useState("");
  const [analysisMessage, setAnalysisMessage] = useState("");
  const [reviewItems, setReviewItems] = useState<ReviewItem[]>([]);
  const [reviewHistory, setReviewHistory] = useState<ReviewDecision[]>([]);
  const [selectedReviewKey, setSelectedReviewKey] = useState<string | null>(null);
  const [pendingDecision, setPendingDecision] = useState<{ item: ReviewItem; decision: "accept" | "ignore" } | null>(null);
  const [reviewAction, setReviewAction] = useState(false);
  const [reviewError, setReviewError] = useState("");
  const [localItems, setLocalItems] = useState<LocalItem[]>([]);
  const [pendingUndo, setPendingUndo] = useState<LocalItem | null>(null);
  const [pendingLifecycle, setPendingLifecycle] = useState<{ item: LocalItem; eventType: "complete" | "snooze" | "reschedule" } | null>(null);
  const [lifecycleDate, setLifecycleDate] = useState("");
  const [localItemEvents, setLocalItemEvents] = useState<LocalItemEvent[]>([]);
  const [focusView, setFocusView] = useState<"today" | "work" | "kids" | "personal">("today");
  const [focusItems, setFocusItems] = useState<LocalItem[]>([]);
  const [sourceItem, setSourceItem] = useState<LocalItemProvenance | null>(null);
  const [home, setHome] = useState<HomeDashboard | null>(null);
  const [homeError, setHomeError] = useState("");
  const [activeSection, setActiveSection] = useState<"home" | "focus" | "review" | "assistant" | "accounts" | "settings" | "ai">("home");
  const [localItemError, setLocalItemError] = useState("");
  const [actionProposals, setActionProposals] = useState<ActionProposal[]>([]);
  const [correspondenceHistory, setCorrespondenceHistory] = useState<CorrespondenceExecutionHistory[]>([]);
  const [automationAudit, setAutomationAudit] = useState<AutomationAuditEntry[]>([]);
  const [pendingProposalDecision, setPendingProposalDecision] = useState<{ proposal: ActionProposal; eventType: "confirm" | "cancel" } | null>(null);
  const [pendingCalendarExecution, setPendingCalendarExecution] = useState<ActionProposal | null>(null);
  const [calendarUpdateCandidates, setCalendarUpdateCandidates] = useState<CalendarUpdateCandidate[]>([]);
  const [pendingCalendarUpdate, setPendingCalendarUpdate] = useState<CalendarUpdateCandidate | null>(null);
  const [calendarUpdateStart, setCalendarUpdateStart] = useState("");
  const [calendarUpdateEnd, setCalendarUpdateEnd] = useState("");
  const [pendingCalendarUpdateExecution, setPendingCalendarUpdateExecution] = useState<ActionProposal | null>(null);
  const [pendingCorrespondenceExecution, setPendingCorrespondenceExecution] = useState<ActionProposal | null>(null);
  const [proposalError, setProposalError] = useState("");
  const [notificationPreviews, setNotificationPreviews] = useState<NotificationPreview[]>([]);
  const [notificationPermission, setNotificationPermission] = useState<"checking" | "granted" | "notGranted">("checking");
  const [automaticPendingCount, setAutomaticPendingCount] = useState<number | null>(null);
  const [notificationAction, setNotificationAction] = useState<"permission" | "testing" | "scheduling" | null>(null);
  const [notificationMessage, setNotificationMessage] = useState("");
  const [familyDisplays, setFamilyDisplays] = useState<FamilyDisplayRecord[]>([]);
  const [displayMessage, setDisplayMessage] = useState("");
  const [displayAction, setDisplayAction] = useState<string | null>(null);
  const [displayService, setDisplayService] = useState<FamilyDisplayServiceStatus | null>(null);
  const [displayBindAddress, setDisplayBindAddress] = useState("");
  const [confirmDisplayEnable, setConfirmDisplayEnable] = useState(false);
  const [displayPairing, setDisplayPairing] = useState<FamilyDisplayPairingChallenge | null>(null);
  const [pendingDisplayRevoke, setPendingDisplayRevoke] = useState<FamilyDisplayRecord | null>(null);
  const [aiCapabilities, setAiCapabilities] = useState<AiCapabilities | null>(null);
  const [aiError, setAiError] = useState("");
  const [aiAction, setAiAction] = useState<"downloading" | "testing" | "evaluating" | "persistentEvaluating" | "removing" | null>(null);
  const [aiProgress, setAiProgress] = useState<AiDownloadProgress | null>(null);
  const [runtimeHealth, setRuntimeHealth] = useState<AiRuntimeHealth | null>(null);
  const [evaluation, setEvaluation] = useState<AiEvaluationReport | null>(null);
  const [persistentEvaluation, setPersistentEvaluation] = useState<AiEvaluationReport | null>(null);

  useEffect(() => {
    api.load().then((value) => { setSettings(value); setState("ready"); }).catch(() => {
      setError("Settings could not be loaded. Your data has not been changed."); setState("error");
    });
  }, [api]);

  useEffect(() => { microsoftApi.status().then(setMicrosoft).catch(() => setConnectionError("Account status could not be loaded.")); }, []);
  useEffect(() => { aiApi.capabilities().then(setAiCapabilities).catch(() => setAiError("Private AI capability detection could not be completed.")); }, []);
  useEffect(() => { reviewApi.list().then(setReviewItems).catch(() => setReviewError("Review items could not be loaded.")); }, []);
  useEffect(() => { reviewApi.history().then(setReviewHistory).catch(() => setReviewError("Review history could not be loaded.")); }, []);
  useEffect(() => { localItemsApi.list().then(setLocalItems).catch(() => setLocalItemError("Local assistant items could not be loaded.")); }, []);
  useEffect(() => { localItemsApi.events().then(setLocalItemEvents).catch(() => setLocalItemError("Local activity history could not be loaded.")); }, []);
  useEffect(() => { actionProposalsApi.list(new Date().toISOString()).then(setActionProposals).catch(() => setProposalError("Automation proposals could not be loaded.")); }, []);
  useEffect(() => { actionProposalsApi.correspondenceHistory().then(setCorrespondenceHistory).catch(() => setProposalError("Correspondence history could not be loaded.")); }, []);
  useEffect(() => { actionProposalsApi.auditHistory().then(setAutomationAudit).catch(() => setProposalError("Automation audit history could not be loaded.")); }, []);
  useEffect(() => {
    if (!microsoft?.accounts.length) { setCalendarUpdateCandidates([]); return; }
    Promise.all(microsoft.accounts.map((account) => actionProposalsApi.calendarUpdateCandidates(account.id)))
      .then((groups) => setCalendarUpdateCandidates(groups.flat()))
      .catch(() => setProposalError("Synchronized calendar events could not be loaded safely."));
  }, [microsoft]);
  useEffect(() => { notificationPreviewsApi.list(localNowRfc3339()).then(setNotificationPreviews).catch(() => setProposalError("Notification previews could not be loaded.")); }, []);
  useEffect(() => { notificationPreviewsApi.permissionStatus().then((permission) => setNotificationPermission(permission === "granted" ? "granted" : "notGranted")).catch(() => setNotificationPermission("notGranted")); }, []);
  useEffect(() => { notificationPreviewsApi.scheduleStatus().then(setAutomaticPendingCount).catch(() => setAutomaticPendingCount(null)); }, []);
  useEffect(() => { familyDisplaysApi.list().then(setFamilyDisplays).catch(() => setDisplayMessage("Family Display records could not be loaded.")); }, []);
  useEffect(() => { familyDisplaysApi.serviceStatus().then((value) => { setDisplayService(value); setDisplayBindAddress(value.bindAddress || ""); }).catch(() => setDisplayMessage("Family Display service status could not be loaded.")); }, []);
  useEffect(() => { const [start, end] = localDayBounds(); localItemsApi.focus(focusView, start, end).then(setFocusItems).catch(() => setLocalItemError("The selected focus view could not be loaded.")); }, [focusView, localItems]);
  useEffect(() => { const [start, end] = localDayBounds(); homeApi.dashboard(start, end).then(setHome).catch(() => setHomeError("Home dashboard could not be loaded.")); }, [localItems, reviewItems, microsoft]);
  useEffect(() => {
    const unlisten = listen<AiDownloadProgress>("ai-model-download-progress", (event) => setAiProgress(event.payload));
    return () => { void unlisten.then((dispose) => dispose()); };
  }, []);

  async function downloadAiModel() {
    setAiAction("downloading"); setAiError(""); setAiProgress(null);
    try { setAiCapabilities(await aiApi.download()); }
    catch (reason) { setAiError(typeof reason === "string" ? reason : "The private model could not be downloaded safely."); }
    finally { setAiAction(null); }
  }

  async function removeAiModel() {
    if (!window.confirm("Remove the downloaded private AI model from this Mac? It can be downloaded again later.")) return;
    setAiAction("removing"); setAiError("");
    try { setAiCapabilities(await aiApi.remove()); setAiProgress(null); }
    catch (reason) { setAiError(typeof reason === "string" ? reason : "The private model could not be removed."); }
    finally { setAiAction(null); }
  }

  async function testAiRuntime() {
    setAiAction("testing"); setAiError(""); setRuntimeHealth(null);
    try { setRuntimeHealth(await aiApi.testRuntime()); }
    catch (reason) { setAiError(typeof reason === "string" ? reason : "The bundled private AI runtime check failed."); }
    finally { setAiAction(null); }
  }

  async function testAiEvaluation() {
    setAiAction("evaluating"); setAiError(""); setEvaluation(null);
    try { setEvaluation(await aiApi.testEvaluation()); }
    catch (reason) { setAiError(typeof reason === "string" ? reason : "The synthetic private AI evaluation failed."); }
    finally { setAiAction(null); }
  }

  async function testPersistentAiEvaluation() {
    setAiAction("persistentEvaluating"); setAiError(""); setPersistentEvaluation(null);
    try { setPersistentEvaluation(await aiApi.testPersistentEvaluation()); }
    catch (reason) { setAiError(typeof reason === "string" ? reason : "The persistent worker evaluation failed safely."); }
    finally { setAiAction(null); }
  }

  async function submit(event: FormEvent) {
    event.preventDefault(); setState("saving"); setError("");
    try { setSettings(await api.save(settings)); setNotificationPreviews(await notificationPreviewsApi.list(localNowRfc3339())); setAutomaticPendingCount(await notificationPreviewsApi.scheduleStatus()); setState("saved"); }
    catch { setError("Settings could not be saved. Check the values and try again."); setState("error"); }
  }

  async function revokeFamilyDisplay(display: FamilyDisplayRecord) {
    setDisplayAction(display.id); setDisplayMessage("");
    try { setFamilyDisplays(await familyDisplaysApi.revoke(display.id)); setPendingDisplayRevoke(null); setDisplayMessage(`${display.displayName} was revoked.`); }
    catch { setDisplayMessage("The display could not be revoked safely."); }
    finally { setDisplayAction(null); }
  }

  async function enableDisplayService() {
    setDisplayAction("service"); setDisplayMessage("");
    try { const value = await familyDisplaysApi.enableService(displayBindAddress.trim()); setDisplayService(value); setConfirmDisplayEnable(false); setDisplayMessage("Encrypted Family Display service started."); }
    catch (reason) { setDisplayMessage(typeof reason === "string" ? reason : "The Family Display service could not be started safely."); }
    finally { setDisplayAction(null); }
  }

  async function disableDisplayService() {
    setDisplayAction("service"); setDisplayMessage("");
    try { setDisplayService(await familyDisplaysApi.disableService()); setDisplayMessage("Family Display service stopped. Paired credentials remain revocable."); }
    catch (reason) { setDisplayMessage(typeof reason === "string" ? reason : "The Family Display service could not be stopped safely."); }
    finally { setDisplayAction(null); }
  }

  async function beginDisplayPairing() {
    setDisplayAction("pairing"); setDisplayMessage("");
    try { setDisplayPairing(await familyDisplaysApi.beginPairing()); setDisplayMessage("A single-use pairing challenge is ready."); }
    catch (reason) { setDisplayMessage(typeof reason === "string" ? reason : "Pairing could not be started safely."); }
    finally { setDisplayAction(null); }
  }

  async function connectMicrosoft() {
    setConnecting(true); setConnectionError("");
    try { await microsoftApi.connect(); setMicrosoft(await microsoftApi.status()); }
    catch (reason) { setConnectionError(typeof reason === "string" ? reason : "Microsoft sign-in could not be completed."); }
    finally { setConnecting(false); }
  }

  async function syncMicrosoft(accountId: string) {
    setAccountAction(accountId); setConnectionError(""); setSyncMessage("");
    try { const result = await microsoftApi.sync(accountId); setMicrosoft(await microsoftApi.status()); setSyncMessage(`Synced ${result.inbox} inbox items, ${result.sent} sent items and ${result.calendar} calendar events.`); }
    catch (reason) { setConnectionError(typeof reason === "string" ? reason : "Microsoft synchronization could not be completed."); }
    finally { setAccountAction(null); }
  }

  async function disconnectMicrosoft(accountId: string) {
    setAccountAction(accountId); setConnectionError("");
    try { await microsoftApi.disconnect(accountId, false); setMicrosoft(await microsoftApi.status()); setSyncMessage("Microsoft account disconnected. Local metadata was retained."); }
    catch { setConnectionError("The Microsoft account could not be disconnected."); }
    finally { setAccountAction(null); }
  }

  async function cancelSync(accountId: string) {
    try { await microsoftApi.cancelSync(accountId); setSyncMessage("Stopping synchronization safely…"); }
    catch { setConnectionError("Synchronization could not be cancelled."); }
  }

  async function deleteLocalData(accountId: string) {
    if (!window.confirm("Delete downloaded Microsoft mail metadata, calendar events and synchronization history from this Mac? The account will remain connected.")) return;
    setAccountAction(accountId); setConnectionError("");
    try { await microsoftApi.deleteLocalData(accountId); setMicrosoft(await microsoftApi.status()); setSyncMessage("Downloaded Microsoft data was deleted from this Mac."); }
    catch (reason) { setConnectionError(typeof reason === "string" ? reason : "Downloaded data could not be deleted."); }
    finally { setAccountAction(null); }
  }

  async function analyzeMessage(accountId: string, providerId: string) {
    setPendingAnalysis(null);
    setAnalyzingMessage(providerId); setConnectionError(""); setAnalysisMessage("");
    try {
      const result = await microsoftApi.analyzeMessage(accountId, providerId);
      const outcome = result.localProjectionCount > 0 ? `Added ${result.localProjectionCount} reversible local item${result.localProjectionCount === 1 ? "" : "s"}` : result.disposition === "review" || result.disposition === "suggestions" ? "Added to Review" : "Analysis saved";
      setAnalysisMessage(`${outcome} · ${titleCase(result.classification.replaceAll("_", " "))} · ${result.suggestionCount} suggestion${result.suggestionCount === 1 ? "" : "s"}. ${result.summary}`);
      setMicrosoft(await microsoftApi.status());
      setReviewItems(await reviewApi.list());
      setLocalItems(await localItemsApi.list());
      setAutomationAudit(await actionProposalsApi.auditHistory());
    } catch (reason) { setConnectionError(typeof reason === "string" ? reason : "The selected message could not be analysed safely."); }
    finally { setAnalyzingMessage(null); }
  }

  async function saveReplyDraft() {
    if (!pendingReplyDraft || !replyDraftText.trim()) return;
    setReviewAction(true); setConnectionError(""); setAnalysisMessage(""); setSyncMessage("");
    try {
      const saved = await microsoftApi.saveReplyDraft(pendingReplyDraft.accountId, pendingReplyDraft.providerId, replyDraftText);
      setReplyDraftText(""); setPendingReplyDraft(null);
      setMicrosoft(await microsoftApi.status());
      setAnalysisMessage(`Reply draft saved privately in macOS Keychain · ${saved.contentBytes} bytes. Sending remains disabled.`);
    } catch (reason) { setConnectionError(typeof reason === "string" ? reason : "The reply draft could not be stored safely."); }
    finally { setReviewAction(false); }
  }

  async function verifyReplyDrafts(accountId: string) {
    setAccountAction(accountId); setConnectionError(""); setSyncMessage("");
    try { const count = await microsoftApi.verifyReplyDrafts(accountId); setSyncMessage(`Verified ${count} private reply draft${count === 1 ? "" : "s"} in macOS Keychain. No content was displayed.`); }
    catch (reason) { setConnectionError(typeof reason === "string" ? reason : "Private reply draft storage could not be verified."); }
    finally { setAccountAction(null); }
  }

  async function queueReplyDraft(accountId: string, providerMessageId: string) {
    setAccountAction(accountId); setConnectionError(""); setSyncMessage("");
    try {
      await microsoftApi.queueReplyDraft(accountId, providerMessageId);
      setActionProposals(await actionProposalsApi.list(new Date().toISOString()));
      setSyncMessage("Private reply draft added for local confirmation. Sending remains disabled.");
    } catch (reason) { setConnectionError(typeof reason === "string" ? reason : "The private reply proposal could not be created safely."); }
    finally { setAccountAction(null); }
  }

  async function decideReview() {
    if (!pendingDecision) return;
    setReviewAction(true); setReviewError("");
    try {
      await reviewApi.decide(pendingDecision.item.accountId, pendingDecision.item.providerId, pendingDecision.decision);
      const [items, history, projected, proposals] = await Promise.all([reviewApi.list(), reviewApi.history(), localItemsApi.list(), actionProposalsApi.list(new Date().toISOString())]);
      setReviewItems(items); setReviewHistory(history); setLocalItems(projected); setActionProposals(proposals); setSelectedReviewKey(null); setPendingDecision(null);
    } catch (reason) { setReviewError(typeof reason === "string" ? reason : "The review decision could not be recorded safely."); }
    finally { setReviewAction(false); }
  }

  async function undoLocalItem() {
    if (!pendingUndo) return;
    setReviewAction(true); setLocalItemError("");
    try { await localItemsApi.undo(pendingUndo.id); setLocalItems(await localItemsApi.list()); setPendingUndo(null); }
    catch (reason) { setLocalItemError(typeof reason === "string" ? reason : "The local item could not be undone safely."); }
    finally { setReviewAction(false); }
  }

  async function transitionLocalItem() {
    if (!pendingLifecycle) return;
    let scheduledAt: string | undefined;
    if (pendingLifecycle.eventType !== "complete") {
      if (!lifecycleDate) { setLocalItemError("Choose a date and time first."); return; }
      scheduledAt = new Date(lifecycleDate).toISOString();
    }
    setReviewAction(true); setLocalItemError("");
    try {
      await localItemsApi.transition(pendingLifecycle.item.id, crypto.randomUUID(), pendingLifecycle.eventType, scheduledAt);
      const [items, events] = await Promise.all([localItemsApi.list(), localItemsApi.events()]);
      setLocalItems(items); setLocalItemEvents(events); setPendingLifecycle(null); setLifecycleDate("");
    } catch (reason) { setLocalItemError(typeof reason === "string" ? reason : "The local lifecycle change could not be recorded safely."); }
    finally { setReviewAction(false); }
  }

  async function showLocalItemSource(item: LocalItem) {
    setLocalItemError("");
    try { setSourceItem(await localItemsApi.provenance(item.id)); }
    catch (reason) { setLocalItemError(typeof reason === "string" ? reason : "Source provenance could not be loaded safely."); }
  }

  async function openLocalItemSource() {
    if (!sourceItem) return;
    setReviewAction(true); setLocalItemError("");
    try { await localItemsApi.openSource(sourceItem.itemId); setSourceItem(null); }
    catch (reason) { setLocalItemError(typeof reason === "string" ? reason : "The source email could not be opened safely."); }
    finally { setReviewAction(false); }
  }

  async function decideActionProposal() {
    if (!pendingProposalDecision) return;
    setReviewAction(true); setProposalError("");
    try {
      await actionProposalsApi.decide(pendingProposalDecision.proposal.proposalKey, crypto.randomUUID(), pendingProposalDecision.eventType);
      setActionProposals(await actionProposalsApi.list(new Date().toISOString()));
      setPendingProposalDecision(null);
    } catch (reason) { setProposalError(typeof reason === "string" ? reason : "The proposal decision could not be recorded safely."); }
    finally { setReviewAction(false); }
  }

  async function executeCalendarProposal() {
    if (!pendingCalendarExecution) return;
    setReviewAction(true); setProposalError("");
    try { const message = await actionProposalsApi.executeCalendar(pendingCalendarExecution.proposalKey, crypto.randomUUID()); setProposalError(message); setPendingCalendarExecution(null); }
    catch (reason) { setProposalError(typeof reason === "string" ? reason : "The calendar event could not be created safely. Check Microsoft Calendar before retrying."); }
    finally { setReviewAction(false); }
  }

  function beginCalendarUpdate(candidate: CalendarUpdateCandidate) {
    setPendingCalendarUpdate(candidate);
    setCalendarUpdateStart(toLocalDateTimeInput(candidate.startAt, 60));
    setCalendarUpdateEnd(toLocalDateTimeInput(candidate.endAt, 60));
    setProposalError("");
  }

  async function queueCalendarUpdate() {
    if (!pendingCalendarUpdate || !calendarUpdateStart || !calendarUpdateEnd) return;
    const start = new Date(calendarUpdateStart);
    const end = new Date(calendarUpdateEnd);
    if (!Number.isFinite(start.valueOf()) || !Number.isFinite(end.valueOf()) || end <= start) { setProposalError("Choose a valid end time after the start time."); return; }
    setReviewAction(true); setProposalError("");
    try {
      await actionProposalsApi.queueCalendarUpdate(pendingCalendarUpdate.accountId, pendingCalendarUpdate.providerId, start.toISOString(), end.toISOString());
      setActionProposals(await actionProposalsApi.list(new Date().toISOString()));
      setPendingCalendarUpdate(null); setCalendarUpdateStart(""); setCalendarUpdateEnd("");
      setProposalError("Calendar update added for local confirmation. Microsoft has not been changed.");
    } catch (reason) { setProposalError(typeof reason === "string" ? reason : "The calendar update could not be prepared safely."); }
    finally { setReviewAction(false); }
  }

  async function executeCalendarUpdateProposal() {
    if (!pendingCalendarUpdateExecution) return;
    setReviewAction(true); setProposalError("");
    try {
      const message = await actionProposalsApi.executeCalendarUpdate(pendingCalendarUpdateExecution.proposalKey, crypto.randomUUID());
      setProposalError(message); setPendingCalendarUpdateExecution(null);
      setActionProposals(await actionProposalsApi.list(new Date().toISOString()));
    } catch (reason) { setProposalError(typeof reason === "string" ? reason : "The calendar update could not be completed safely. Check Microsoft Calendar before retrying."); }
    finally { setReviewAction(false); }
  }

  async function executeCorrespondenceProposal() {
    if (!pendingCorrespondenceExecution) return;
    setReviewAction(true); setProposalError("");
    try {
      const message = await actionProposalsApi.executeCorrespondence(pendingCorrespondenceExecution.proposalKey, crypto.randomUUID());
      setProposalError(message); setPendingCorrespondenceExecution(null);
      setActionProposals(await actionProposalsApi.list(new Date().toISOString()));
    } catch (reason) { setProposalError(typeof reason === "string" ? reason : "The reply could not be completed safely. Check Microsoft Sent Items before taking any further action."); }
    finally { setCorrespondenceHistory(await actionProposalsApi.correspondenceHistory().catch(() => correspondenceHistory)); setReviewAction(false); }
  }

  async function enableNotifications() {
    setNotificationAction("permission"); setNotificationMessage("");
    try { const permission = await notificationPreviewsApi.requestPermission(); setNotificationPermission(permission === "granted" ? "granted" : "notGranted"); setNotificationMessage(permission === "granted" ? "macOS notification permission is available." : "macOS did not grant notification permission. You can change this in System Settings → Notifications."); }
    catch { setNotificationPermission("notGranted"); setNotificationMessage("Notification permission could not be requested."); }
    finally { setNotificationAction(null); }
  }

  async function sendTestNotification() {
    setNotificationAction("testing"); setNotificationMessage("");
    try { const receipt = await notificationPreviewsApi.sendGenericTest(crypto.randomUUID()); setNotificationMessage(receipt === "delivered" ? "Delivered: macOS placed the generic test in Notification Center." : "Accepted by macOS, but no delivery receipt was visible. Check Focus and System Settings → Notifications."); }
    catch (reason) { setNotificationMessage(typeof reason === "string" ? reason : "The generic test notification could not be sent."); }
    finally { setNotificationAction(null); }
  }

  async function scheduleTestNotification() {
    setNotificationAction("scheduling"); setNotificationMessage("");
    try { const deliverAt = await notificationPreviewsApi.scheduleGenericTest(crypto.randomUUID()); setNotificationMessage(`Scheduled test accepted by macOS for ${new Date(deliverAt).toLocaleTimeString()}. It contains no personal or email content.`); }
    catch (reason) { setNotificationMessage(typeof reason === "string" ? reason : "The scheduled test notification could not be created."); }
    finally { setNotificationAction(null); }
  }

  const disabled = state === "loading" || state === "saving";
  const selectedReview = reviewItems.find((item) => reviewKey(item) === selectedReviewKey) ?? reviewItems[0];
  return <main>
    <header><span className="eyebrow">PRIVATE · ON THIS MAC</span><h1>Personal Assistant</h1><p>Your household control centre.</p></header>
    <nav className="app-navigation" aria-label="Personal Assistant sections">{([['home','Home'],['focus','Focus'],['review','Review'],['assistant','Assistant'],['accounts','Accounts'],['settings','Settings'],['ai','Private AI']] as const).map(([section, label]) => <button type="button" key={section} aria-current={activeSection === section ? "page" : undefined} onClick={() => setActiveSection(section)}>{label}{section === "review" && home && home.reviewCount > 0 ? <span className="nav-count" aria-label={`${home.reviewCount} pending`}>{home.reviewCount}</span> : null}</button>)}</nav>
    {activeSection === "home" && <section id="panel-home" className="home-dashboard workspace-panel" aria-labelledby="home-title">
      <div><h2 id="home-title">Home</h2><p>A local snapshot of what needs attention.</p></div>
      {homeError && <output aria-live="polite">{homeError}</output>}
      {home && <><div className="home-counts"><div><strong>{home.todoCount}</strong><span>To Do</span></div><div><strong>{home.waitingCount}</strong><span>Waiting</span></div><div><strong>{home.calendarCount}</strong><span>Calendar</span></div><div><strong>{home.reviewCount}</strong><span>Review</span></div></div><div className="home-columns"><article><h3>Today</h3>{home.today.length === 0 ? <p>Nothing scheduled for today.</p> : <ul>{home.today.map((item) => <li key={item.id}><strong>{item.title}</strong><small>{titleCase(item.category)} · {localItemDate(item)}</small></li>)}</ul>}</article><article><h3>Sync health</h3><p>{home.sync.connectedAccounts === 0 ? "No account connected." : home.sync.attentionAccounts === 0 ? `${home.sync.healthyAccounts} account${home.sync.healthyAccounts === 1 ? "" : "s"} healthy.` : `${home.sync.attentionAccounts} account${home.sync.attentionAccounts === 1 ? "" : "s"} need attention.`}</p>{home.sync.lastSyncAt && <small>Last successful sync: {home.sync.lastSyncAt}</small>}</article></div></>}
    </section>}
    {activeSection === "settings" && <section id="panel-settings" className="workspace-panel" aria-labelledby="settings-title">
      <div><h2 id="settings-title">General settings</h2><p>These preferences stay in the local application database.</p></div>
      <form onSubmit={submit}>
        <label>Household name<input required maxLength={80} value={settings.householdName} disabled={disabled} onChange={(e) => setSettings({ ...settings, householdName: e.target.value })} /></label>
        <label>Appearance<select value={settings.theme} disabled={disabled} onChange={(e) => setSettings({ ...settings, theme: e.target.value as Theme })}><option value="system">Use system setting</option><option value="light">Light</option><option value="dark">Dark</option></select></label>
        <label>Automation policy<select value={settings.automationPolicy} disabled={disabled} onChange={(e) => setSettings({ ...settings, automationPolicy: e.target.value as AutomationPolicy })}><option value="conservative">Conservative — always ask before changes</option><option value="balanced">Balanced — safe default</option><option value="assistant">Assistant — routine administration after safeguards</option></select><small>This records your policy only. Automatic actions remain disabled until each action type passes its safety gate.</small></label>
        <label className="check"><input type="checkbox" checked={settings.notificationDeliveryEnabled} disabled={disabled || notificationPermission !== "granted"} onChange={(e) => setSettings({ ...settings, notificationDeliveryEnabled: e.target.checked })} /><span><strong>Allow automatic local notification delivery</strong><small>Off by default. When saved, only enabled local notification types are scheduled; turning this off cancels pending automatic deliveries.</small></span></label>
        <p className="notice">Automatic requests currently pending in macOS: <strong>{automaticPendingCount ?? "Unavailable"}</strong>. Saving settings reconciles this count immediately.</p>
        <div className="actions"><button type="button" className="secondary" disabled={notificationAction !== null || notificationPermission !== "granted"} onClick={scheduleTestNotification}>{notificationAction === "scheduling" ? "Scheduling test…" : "Schedule one-minute test"}</button></div>
        <fieldset><legend>Notification preferences</legend><p>All notification types default off. Automatic delivery also requires the separate consent above.</p><label className="check"><input type="checkbox" checked={settings.urgentAlertsEnabled} disabled={disabled} onChange={(e) => setSettings({ ...settings, urgentAlertsEnabled: e.target.checked })} /><span><strong>Urgent alerts</strong><small>May bypass quiet hours after notification delivery is separately enabled.</small></span></label><label className="check"><input type="checkbox" checked={settings.appointmentRemindersEnabled} disabled={disabled} onChange={(e) => setSettings({ ...settings, appointmentRemindersEnabled: e.target.checked })} /><span><strong>Appointment reminders</strong></span></label><label className="check"><input type="checkbox" checked={settings.morningSummaryEnabled} disabled={disabled} onChange={(e) => setSettings({ ...settings, morningSummaryEnabled: e.target.checked })} /><span><strong>Morning summary</strong></span></label><label className="check"><input type="checkbox" checked={settings.eveningSummaryEnabled} disabled={disabled} onChange={(e) => setSettings({ ...settings, eveningSummaryEnabled: e.target.checked })} /><span><strong>Evening summary</strong></span></label><div className="time-grid"><label>Quiet hours start<input type="time" value={minuteToTime(settings.quietHoursStartMinute)} disabled={disabled} onChange={(e) => setSettings({ ...settings, quietHoursStartMinute: timeToMinute(e.target.value) })} /></label><label>Quiet hours end<input type="time" value={minuteToTime(settings.quietHoursEndMinute)} disabled={disabled} onChange={(e) => setSettings({ ...settings, quietHoursEndMinute: timeToMinute(e.target.value) })} /></label></div><div className="actions"><button type="button" disabled={notificationAction !== null || notificationPermission === "granted"} onClick={enableNotifications}>{notificationAction === "permission" ? "Waiting for macOS…" : notificationPermission === "granted" ? "Permission available" : "Enable notifications"}</button><button type="button" className="secondary" disabled={notificationAction !== null || notificationPermission !== "granted"} onClick={sendTestNotification}>{notificationAction === "testing" ? "Sending test…" : "Send generic test"}</button></div>{notificationMessage && <output aria-live="polite">{notificationMessage}</output>}<small>The web interface has no permission to choose notification content. The native test uses a fixed generic message.</small></fieldset>
        <fieldset><legend>Family Displays</legend><p>Share privacy-filtered local items over an encrypted, read-only connection. The service is off by default and never configures your router.</p><label>Mac private network address<input placeholder="192.168.1.20:8765" value={displayBindAddress} disabled={displayAction !== null || displayService?.running} onChange={(event) => setDisplayBindAddress(event.target.value)} /><small>Use this Mac’s private IPv4 or IPv6 address and port 8765. Wildcard and public addresses are rejected.</small></label><p className="notice">Service: <strong>{displayService?.running ? "Running" : displayService?.enabled ? "Configured but not running" : "Off"}</strong>{displayService?.certificateSha256 && <> · certificate fingerprint <code>{displayService.certificateSha256.slice(0, 12)}…</code></>}</p><div className="actions">{displayService?.running ? <><button type="button" className="secondary" disabled={displayAction !== null} onClick={disableDisplayService}>{displayAction === "service" ? "Stopping…" : "Stop service"}</button><button type="button" disabled={displayAction !== null} onClick={beginDisplayPairing}>{displayAction === "pairing" ? "Creating challenge…" : "Pair a display"}</button></> : <button type="button" disabled={displayAction !== null || !displayBindAddress.trim()} onClick={() => setConfirmDisplayEnable(true)}>{displayAction === "service" ? "Starting…" : "Enable encrypted service"}</button>}</div>{displayPairing && <p className="notice"><strong>Pairing code: <code>{displayPairing.code}</code></strong><br /><small>Single use · expires {new Date(displayPairing.expiresAtUnix * 1000).toLocaleTimeString()}. Only enter it in the native Family Display setup.</small></p>}{familyDisplays.length === 0 ? <p>No family displays are paired.</p> : <ul className="settings-list">{familyDisplays.map((display) => <li key={display.id}><span><strong>{display.displayName}</strong><small>{display.revoked ? "Revoked" : display.lastSeenAt ? `Last seen: ${display.lastSeenAt}` : "Paired · not yet seen"}</small></span>{!display.revoked && <button type="button" className="secondary" disabled={displayAction !== null} onClick={() => setPendingDisplayRevoke(display)}>{displayAction === display.id ? "Revoking…" : "Revoke"}</button>}</li>)}</ul>}<small>The code is not a credential. The final 256-bit read-only credential is delivered directly over pinned TLS and never enters this webview.</small>{displayMessage && <output aria-live="polite">{displayMessage}</output>}</fieldset>
        {displayPairing && <FamilyDisplaySetupDetails challenge={displayPairing} />}
        <label className="check"><input type="checkbox" checked={settings.launchAtLogin} disabled={disabled} onChange={(e) => setSettings({ ...settings, launchAtLogin: e.target.checked })} /><span><strong>Launch at login</strong><small>Preference stored; OS registration is planned after the foundation milestone.</small></span></label>
        <label className="check"><input type="checkbox" checked={settings.storeCompleteEmailContent} disabled={disabled} onChange={(e) => setSettings({ ...settings, storeCompleteEmailContent: e.target.checked })} /><span><strong>Store complete email content locally</strong><small>Off by default. Enabling this records your preference; bodies will only be retained once encrypted content storage is implemented and separately confirmed.</small></span></label>
        <label className="check"><input type="checkbox" checked={settings.localEmailAnalysisEnabled} disabled={disabled} onChange={(e) => setSettings({ ...settings, localEmailAnalysisEnabled: e.target.checked })} /><span><strong>Allow private local email analysis</strong><small>Off by default. Requires this exact model, runtime and persistent evaluation to pass. This consent does not enable body storage or automatic mailbox processing yet.</small></span></label>
        <div className="actions"><button disabled={disabled}>{state === "saving" ? "Saving…" : "Save settings"}</button><output aria-live="polite">{state === "saved" ? "Saved on this Mac" : error}</output></div>
      </form>
      {confirmDisplayEnable && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="display-enable-title"><strong id="display-enable-title">Enable the encrypted Family Display service?</strong><p>This will listen only on <code>{displayBindAddress.trim()}</code>. A native TLS private key will be stored in macOS Keychain. No email content, provider identifiers, or private item titles are exposed.</p><div className="actions"><button type="button" disabled={displayAction !== null} onClick={enableDisplayService}>{displayAction === "service" ? "Starting securely…" : "Confirm and enable"}</button><button type="button" className="secondary" disabled={displayAction !== null} onClick={() => setConfirmDisplayEnable(false)}>Cancel</button></div></div>}
      {pendingDisplayRevoke && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="display-revoke-title"><strong id="display-revoke-title">Revoke “{pendingDisplayRevoke.displayName}”?</strong><p>Its read-only credential will stop working immediately. The audit record remains and re-pairing will be required.</p><div className="actions"><button type="button" disabled={displayAction !== null} onClick={() => revokeFamilyDisplay(pendingDisplayRevoke)}>{displayAction === pendingDisplayRevoke.id ? "Revoking…" : "Confirm revocation"}</button><button type="button" className="secondary" disabled={displayAction !== null} onClick={() => setPendingDisplayRevoke(null)}>Cancel</button></div></div>}
    </section>}
    {activeSection === "accounts" && <section id="panel-accounts" className="accounts workspace-panel" aria-labelledby="accounts-title">
      <div><h2 id="accounts-title">Microsoft 365 / Outlook</h2><p>Read-only mail and calendar access. Authentication tokens are stored in macOS Keychain.</p></div>
      {microsoft?.accounts.map((account) => <div className="account" key={account.id}><span><strong>{account.displayName}</strong><small>{account.emailAddress}</small><small>{account.lastSync ? `Last sync: ${account.lastSync.outcome} · ${account.lastSync.itemCount} items` : "Not synchronized yet"}</small>{account.recentMessages.length > 0 && <span className="message-list"><small>Recent synchronized messages</small>{account.recentMessages.map((message) => <span className="message-row" key={message.providerId}><span><strong>{message.subject || "Untitled message"}</strong><small>{message.analyzed ? "Analysis saved" : message.occurredAt || "Date unavailable"}</small></span><span className="item-actions"><button type="button" className="secondary" disabled={message.analyzed || !settings.localEmailAnalysisEnabled || analyzingMessage !== null || accountAction !== null} onClick={() => setPendingAnalysis({ accountId: account.id, providerId: message.providerId, subject: message.subject })}>{analyzingMessage === message.providerId ? "Analysing privately…" : message.analyzed ? "Analysis saved" : "Analyse privately"}</button><button type="button" className="secondary" disabled={message.hasReplyDraft || accountAction !== null || reviewAction} onClick={() => { setPendingReplyDraft({ accountId: account.id, providerId: message.providerId, subject: message.subject }); setReplyDraftText(""); }}>{message.hasReplyDraft ? "Draft saved" : "Draft reply"}</button></span></span>)}</span>}</span><div className="account-actions"><b>Connected</b>{accountAction === account.id ? <button className="secondary" type="button" onClick={() => cancelSync(account.id)}>Cancel sync</button> : <button type="button" onClick={() => syncMicrosoft(account.id)}>Sync now</button>}<button className="secondary" type="button" disabled={accountAction === account.id} onClick={() => deleteLocalData(account.id)}>Delete local data</button><button className="secondary" type="button" disabled={accountAction === account.id} onClick={() => disconnectMicrosoft(account.id)}>Disconnect</button></div></div>)}
      {microsoft?.accounts.filter((account) => account.recentMessages.some((message) => message.hasReplyDraft)).map((account) => <button type="button" className="secondary" key={`verify-drafts-${account.id}`} disabled={accountAction !== null} onClick={() => verifyReplyDrafts(account.id)}>Verify private draft storage</button>)}
      {microsoft?.accounts.filter((account) => account.recentMessages.some((message) => message.hasReplyDraft)).map((account) => <button type="button" className="secondary" key={`repair-drafts-${account.id}`} disabled={accountAction !== null || reviewAction} onClick={() => { const message = account.recentMessages.find((candidate) => candidate.hasReplyDraft); if (message) { setPendingReplyDraft({ accountId: account.id, providerId: message.providerId, subject: message.subject }); setReplyDraftText(""); } }}>Replace unavailable private draft</button>)}
      {microsoft?.accounts.filter((account) => account.recentMessages.some((message) => message.hasReplyDraft)).map((account) => <button type="button" key={`confirm-draft-${account.id}`} disabled={accountAction !== null || reviewAction} onClick={() => { const message = account.recentMessages.find((candidate) => candidate.hasReplyDraft); if (message) void queueReplyDraft(account.id, message.providerId); }}>Add private draft for confirmation</button>)}
      {microsoft && microsoft.accounts.length === 0 && <button type="button" disabled={!microsoft.configured || connecting} onClick={connectMicrosoft}>{connecting ? "Waiting for Microsoft…" : "Connect Microsoft account"}</button>}
      {microsoft && !microsoft.configured && <p className="notice">Microsoft connection requires the product’s registered application client ID at build time.</p>}
      {pendingAnalysis && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="analysis-confirmation-title" aria-describedby="analysis-confirmation-description"><strong id="analysis-confirmation-title">Analyse “{pendingAnalysis.subject || "Untitled message"}” privately?</strong><p id="analysis-confirmation-description">The message body will be retrieved transiently, processed only by the qualified model on this Mac, and will not be stored.</p><div className="actions"><button type="button" onClick={() => analyzeMessage(pendingAnalysis.accountId, pendingAnalysis.providerId)}>Confirm private analysis</button><button type="button" className="secondary" onClick={() => setPendingAnalysis(null)}>Cancel</button></div></div>}
      {pendingReplyDraft && <div className="analysis-confirmation" role="dialog" aria-labelledby="reply-draft-title"><strong id="reply-draft-title">Draft a reply to “{pendingReplyDraft.subject || "Untitled message"}”</strong><p>The text will be stored in macOS Keychain and bound to this source message. It will not be sent, uploaded to an AI service, or stored in SQLite.</p><label>Reply text<textarea required maxLength={16384} rows={7} value={replyDraftText} onChange={(event) => setReplyDraftText(event.target.value)} /></label><div className="actions"><button type="button" disabled={reviewAction || !replyDraftText.trim()} onClick={saveReplyDraft}>{reviewAction ? "Saving privately…" : "Save private draft"}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => { setPendingReplyDraft(null); setReplyDraftText(""); }}>Cancel</button></div></div>}
      {connectionError && <output aria-live="polite">{connectionError}</output>}
      {syncMessage && <output aria-live="polite">{syncMessage}</output>}
      {analysisMessage && <output aria-live="polite">{analysisMessage}</output>}
    </section>}
    {activeSection === "review" && <section id="panel-review" className="review workspace-panel" aria-labelledby="review-title">
      <div><h2 id="review-title">Review</h2><p>Items the local rules engine has held for your decision. Suggestions shown here cannot execute actions.</p></div>
      {reviewItems.length === 0 && !reviewError && <p className="empty-state">Nothing needs review.</p>}
      {reviewError && <output aria-live="polite">{reviewError}</output>}
      {reviewItems.length > 0 && <div className="review-layout"><div className="review-list" role="list" aria-label="Items requiring review">{reviewItems.map((item) => <button type="button" role="listitem" className={reviewKey(item) === reviewKey(selectedReview) ? "review-list-item selected" : "review-list-item"} key={reviewKey(item)} onClick={() => setSelectedReviewKey(reviewKey(item))}><strong>{item.subject || "Untitled message"}</strong><small>{titleCase(item.classification)} · {titleCase(item.urgency)} urgency</small><span>{item.summary}</span></button>)}</div>{selectedReview && <article className="review-detail"><span className="eyebrow">NEEDS YOUR REVIEW</span><h3>{selectedReview.subject || "Untitled message"}</h3><p>{selectedReview.summary}</p><dl><div><dt>Classification</dt><dd>{titleCase(selectedReview.classification)} · {Math.round(selectedReview.classificationConfidence * 100)}% confidence</dd></div><div><dt>Urgency</dt><dd>{titleCase(selectedReview.urgency)}</dd></div>{selectedReview.senderName && <div><dt>From</dt><dd>{selectedReview.senderName}</dd></div>}</dl><h4>Why it needs review</h4><ul>{selectedReview.reviewReasons.map((reason) => <li key={reason}>{reviewReasonLabel(reason)}</li>)}</ul><h4>Suggested next step{selectedReview.suggestions.length === 1 ? "" : "s"}</h4>{selectedReview.suggestions.length === 0 ? <p>No action was suggested.</p> : <ul>{selectedReview.suggestions.map((suggestion, index) => <li key={`${suggestion.kind}-${index}`}>{suggestionLabel(suggestion)}</li>)}</ul>}<div className="review-actions"><button type="button" onClick={() => setPendingDecision({ item: selectedReview, decision: "accept" })}>Accept suggestion</button><button type="button" className="secondary" onClick={() => setPendingDecision({ item: selectedReview, decision: "ignore" })}>Ignore</button></div><small className="integrity">A decision records your choice locally; it does not execute the suggestion.</small></article>}</div>}
      {pendingDecision && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="decision-title" aria-describedby="decision-description"><strong id="decision-title">{pendingDecision.decision === "accept" ? "Accept" : "Ignore"} this suggestion?</strong><p id="decision-description">This records a local audit decision for “{pendingDecision.item.subject || "Untitled message"}”. It will not create a task, change a calendar, send a reply, or modify Microsoft data.</p><div className="actions"><button type="button" disabled={reviewAction} onClick={decideReview}>{reviewAction ? "Recording…" : `Confirm ${pendingDecision.decision}`}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => setPendingDecision(null)}>Cancel</button></div></div>}
      {reviewHistory.length > 0 && <div className="review-history"><h3>Recent decisions</h3><ul>{reviewHistory.map((item) => <li key={reviewKey(item)}><span><strong>{item.subject || "Untitled message"}</strong><small>{item.decidedAt}</small></span><b>{item.decision === "accept" ? "Accepted" : "Ignored"}</b></li>)}</ul></div>}
    </section>}
    {activeSection === "focus" && <section id="panel-focus" className="focus-views workspace-panel" aria-labelledby="focus-views-title">
      <div><h2 id="focus-views-title">Focus</h2><p>Deterministic local views over accepted assistant items.</p></div>
      <div className="focus-tabs" role="tablist" aria-label="Focus view">{(["today", "work", "kids", "personal"] as const).map((view) => <button type="button" role="tab" aria-selected={focusView === view} className={focusView === view ? "selected" : ""} key={view} onClick={() => setFocusView(view)}>{titleCase(view)}</button>)}</div>
      <div role="tabpanel" aria-label={`${titleCase(focusView)} items`} className="focus-panel">{focusItems.length === 0 ? <p>Nothing in {titleCase(focusView)}.</p> : <ul>{focusItems.map((item) => <li key={item.id}><span><strong>{item.title}</strong><small>{item.kind === "waiting_for" ? "Waiting For" : titleCase(item.kind)} · {titleCase(item.category)}</small></span>{localItemDate(item) && <time>{localItemDate(item)}</time>}</li>)}</ul>}</div>
    </section>}
    {activeSection === "assistant" && <section id="panel-assistant" className="assistant-views workspace-panel" aria-labelledby="assistant-views-title">
      <div><h2 id="assistant-views-title">Assistant views</h2><p>Local suggestions remain inert until you explicitly confirm and execute a supported action.</p></div>
      <div className="review-history"><h3>Change a synchronized calendar event</h3>{calendarUpdateCandidates.length === 0 ? <p>No synchronized, update-eligible events are available. Sync the Microsoft account first.</p> : <ul>{calendarUpdateCandidates.map((candidate) => <li key={`${candidate.accountId}:${candidate.providerId}`}><span><strong>{candidate.subject}</strong><small>{formatCalendarCandidate(candidate.startAt)}–{formatCalendarCandidate(candidate.endAt)}</small></span><button type="button" className="secondary" onClick={() => beginCalendarUpdate(candidate)}>Propose time change</button></li>)}</ul>}</div>
      <div className="review-history"><h3>Automation proposals</h3>{actionProposals.length === 0 ? <p>No automation proposals. Automatic actions remain disabled.</p> : <ul>{actionProposals.map((proposal) => <li key={proposal.proposalKey}><span><strong>{proposal.displayLabel}</strong><small>{titleCase(proposal.actionKind.replaceAll("_", " "))} · {titleCase(proposal.policy)} · expires {proposal.expiresAt}</small></span><div className="item-actions"><b>{titleCase(proposal.state)}</b>{proposal.state === "pending" && <><button type="button" onClick={() => setPendingProposalDecision({ proposal, eventType: "confirm" })}>Confirm locally</button><button type="button" className="secondary" onClick={() => setPendingProposalDecision({ proposal, eventType: "cancel" })}>Cancel</button></>}{proposal.state === "confirmed" && proposal.actionKind === "calendar_create" && <button type="button" onClick={() => setPendingCalendarExecution(proposal)}>Create calendar event</button>}{proposal.state === "confirmed" && proposal.actionKind === "calendar_update" && <button type="button" onClick={() => setPendingCalendarUpdateExecution(proposal)}>Update calendar event</button>}{proposal.state === "confirmed" && proposal.actionKind === "correspondence" && <button type="button" onClick={() => setPendingCorrespondenceExecution(proposal)}>Send reply</button>}</div></li>)}</ul>}</div>
      {correspondenceHistory.length > 0 && <div className="review-history"><h3>Correspondence history</h3><ul>{correspondenceHistory.map((entry, index) => <li key={`${entry.createdAt}-${index}`}><span><strong>{entry.displayLabel}</strong><small>{correspondenceOutcomeLabel(entry)} · {entry.createdAt}</small></span><b>{titleCase(entry.outcome)}</b></li>)}</ul><small className="integrity">Content-free local audit history. Reply text, recipients, and message bodies are not stored here.</small></div>}
      {automationAudit.length > 0 && <div className="review-history"><h3>Automation policy history</h3><ul>{automationAudit.map((entry, index) => <li key={`${entry.createdAt}-${index}`}><span><strong>{titleCase(entry.actionKind.replaceAll("_", " "))}</strong><small>{titleCase(entry.policy)} · {titleCase(entry.reasonCode.replaceAll("_", " "))} · {entry.createdAt}</small></span><b>{titleCase(entry.decision)}</b></li>)}</ul><small className="integrity">Content-free policy decisions only. This history cannot execute actions.</small></div>}
      <div className="review-history"><h3>Notification previews</h3>{notificationPreviews.length === 0 ? <p>No previews with the current default-off preferences. Nothing will be delivered.</p> : <ul>{notificationPreviews.map((preview) => <li key={preview.idempotencyKey}><span><strong>{preview.title}</strong><small>{preview.body}</small><small>{titleCase(preview.kind.replaceAll("_", " "))} · {preview.deliverAt}{preview.quietHoursApplied ? " · shifted by quiet hours" : ""}</small></span><b>Preview only</b></li>)}</ul>}</div>
      {proposalError && <output aria-live="polite">{proposalError}</output>}
      {pendingProposalDecision && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="proposal-decision-title" aria-describedby="proposal-decision-description"><strong id="proposal-decision-title">{pendingProposalDecision.eventType === "confirm" ? "Confirm" : "Cancel"} “{pendingProposalDecision.proposal.displayLabel}”?</strong><p id="proposal-decision-description">This appends a local audit event only. It will not execute the proposal, create or change a calendar item, send correspondence, or deliver a notification.</p><div className="actions"><button type="button" disabled={reviewAction} onClick={decideActionProposal}>{reviewAction ? "Recording…" : `Record ${pendingProposalDecision.eventType}`}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => setPendingProposalDecision(null)}>Back</button></div></div>}
      {pendingCalendarUpdate && <div className="analysis-confirmation" role="dialog" aria-labelledby="calendar-update-proposal-title"><strong id="calendar-update-proposal-title">Propose a new time for “{pendingCalendarUpdate.subject}”</strong><p>Current time: {formatCalendarCandidate(pendingCalendarUpdate.startAt)}–{formatCalendarCandidate(pendingCalendarUpdate.endAt)}. This step only creates an inert local proposal.</p><label>New start<input type="datetime-local" value={calendarUpdateStart} onChange={(event) => setCalendarUpdateStart(event.target.value)} /></label><label>New end<input type="datetime-local" value={calendarUpdateEnd} onChange={(event) => setCalendarUpdateEnd(event.target.value)} /></label><div className="actions"><button type="button" disabled={reviewAction} onClick={queueCalendarUpdate}>{reviewAction ? "Preparing…" : "Add for confirmation"}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => setPendingCalendarUpdate(null)}>Cancel</button></div></div>}
      {pendingCalendarExecution && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="calendar-execution-title" aria-describedby="calendar-execution-description"><strong id="calendar-execution-title">Create “{pendingCalendarExecution.displayLabel}” in Microsoft Calendar?</strong><p id="calendar-execution-description">This will make one external change now. It will not send email or modify the source message. If the network result is uncertain, the app will stop and ask you to check Calendar rather than retry automatically.</p><div className="actions"><button type="button" disabled={reviewAction} onClick={executeCalendarProposal}>{reviewAction ? "Creating…" : "Create event now"}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => setPendingCalendarExecution(null)}>Cancel</button></div></div>}
      {pendingCalendarUpdateExecution && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="calendar-update-execution-title" aria-describedby="calendar-update-execution-description"><strong id="calendar-update-execution-title">Update “{pendingCalendarUpdateExecution.displayLabel}” in Microsoft Calendar?</strong><p id="calendar-update-execution-description">This will make one external change now using the synchronized event version. If it changed elsewhere or the network result is uncertain, the app will stop without an automatic retry.</p><div className="actions"><button type="button" disabled={reviewAction} onClick={executeCalendarUpdateProposal}>{reviewAction ? "Updating…" : "Update event now"}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => setPendingCalendarUpdateExecution(null)}>Cancel</button></div></div>}
      {pendingCorrespondenceExecution && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="correspondence-execution-title" aria-describedby="correspondence-execution-description"><strong id="correspondence-execution-title">Send “{pendingCorrespondenceExecution.displayLabel}” as a Microsoft reply?</strong><p id="correspondence-execution-description">This immediately sends the exact Keychain-stored draft to the participants of its source message. The app cannot change recipients. If the network result is uncertain, it stops without retrying; check Sent Items before taking any further action.</p><div className="actions"><button type="button" disabled={reviewAction} onClick={executeCorrespondenceProposal}>{reviewAction ? "Sending once…" : "Send reply now"}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => setPendingCorrespondenceExecution(null)}>Cancel</button></div></div>}
      {localItemError && <output aria-live="polite">{localItemError}</output>}
      <div className="local-view-grid">{(["task", "waiting_for", "appointment"] as const).map((kind) => <article className="local-view" key={kind}><h3>{kind === "task" ? "To Do" : kind === "waiting_for" ? "Waiting For" : "Calendar"}</h3>{localItems.filter((item) => item.kind === kind && item.status === "active" && item.lifecycleState !== "completed").length === 0 ? <p>Nothing here yet.</p> : <ul>{localItems.filter((item) => item.kind === kind && item.status === "active" && item.lifecycleState !== "completed").map((item) => <li key={item.id}><span><strong>{item.title}</strong><small>{titleCase(item.category)}{localItemDate(item) ? ` · ${localItemDate(item)}` : ""}{item.lifecycleState === "snoozed" ? " · Snoozed" : ""}</small></span><div className="item-actions"><button type="button" onClick={() => setPendingLifecycle({ item, eventType: "complete" })}>Complete</button><button type="button" onClick={() => setPendingLifecycle({ item, eventType: "snooze" })}>Snooze</button><button type="button" onClick={() => setPendingLifecycle({ item, eventType: "reschedule" })}>Reschedule</button><button type="button" onClick={() => showLocalItemSource(item)}>Source</button><button type="button" className="secondary" onClick={() => setPendingUndo(item)}>Undo</button></div></li>)}</ul>}</article>)}</div>
      {sourceItem && <div className="source-detail" role="dialog" aria-labelledby="source-title"><span className="eyebrow">SANITIZED PROVENANCE</span><strong id="source-title">{sourceItem.sourceSubject || "Untitled source email"}</strong><dl><div><dt>Provider</dt><dd>{titleCase(sourceItem.provider)}</dd></div>{sourceItem.sourceSender && <div><dt>From</dt><dd>{sourceItem.sourceSender}</dd></div>}{sourceItem.sourceOccurredAt && <div><dt>Received</dt><dd>{sourceItem.sourceOccurredAt}</dd></div>}</dl><p>No message body or provider URL is exposed to this interface.</p><div className="actions"><button type="button" disabled={!sourceItem.sourceAvailable || reviewAction} onClick={openLocalItemSource}>{reviewAction ? "Opening…" : sourceItem.sourceAvailable ? "Open source email" : "Source unavailable"}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => setSourceItem(null)}>Close</button></div></div>}
      {pendingLifecycle && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="lifecycle-title" aria-describedby="lifecycle-description"><strong id="lifecycle-title">{titleCase(pendingLifecycle.eventType)} “{pendingLifecycle.item.title}”?</strong><p id="lifecycle-description">This changes only the local assistant item and appends an audit event. Microsoft data will not be modified.</p>{pendingLifecycle.eventType !== "complete" && <label>New local date and time<input type="datetime-local" value={lifecycleDate} onChange={(event) => setLifecycleDate(event.target.value)} /></label>}<div className="actions"><button type="button" disabled={reviewAction} onClick={transitionLocalItem}>{reviewAction ? "Recording…" : `Confirm ${pendingLifecycle.eventType}`}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => { setPendingLifecycle(null); setLifecycleDate(""); }}>Cancel</button></div></div>}
      {pendingUndo && <div className="analysis-confirmation" role="alertdialog" aria-labelledby="undo-title" aria-describedby="undo-description"><strong id="undo-title">Undo “{pendingUndo.title}”?</strong><p id="undo-description">The local projection will be marked undone and removed from its active view. Its source provenance and accepted Review decision will remain in local history.</p><div className="actions"><button type="button" disabled={reviewAction} onClick={undoLocalItem}>{reviewAction ? "Undoing…" : "Confirm undo"}</button><button type="button" className="secondary" disabled={reviewAction} onClick={() => setPendingUndo(null)}>Cancel</button></div></div>}
      {localItems.some((item) => item.status === "undone") && <div className="undo-history"><h3>Recently undone</h3><ul>{localItems.filter((item) => item.status === "undone").map((item) => <li key={item.id}><span>{item.title}</span><small>{titleCase(item.category)} · undone {item.undoneAt}</small></li>)}</ul></div>}
      {localItemEvents.length > 0 && <div className="undo-history"><h3>Local activity</h3><ul>{localItemEvents.map((event) => <li key={event.id}><span>{titleCase(event.eventType)} · item {event.itemId}</span><small>{event.previousState} → {event.newState} · {event.createdAt}</small></li>)}</ul></div>}
    </section>}
    {activeSection === "ai" && <section id="panel-ai" className="ai-settings workspace-panel" aria-labelledby="ai-title">
      <div><h2 id="ai-title">Private AI model</h2><p>Email analysis will run locally on this Mac. No email content will be sent to an external LLM.</p></div>
      {aiCapabilities && <>
        <dl className="capability-grid">
          <div><dt>Processor</dt><dd>{aiCapabilities.hardware.appleSilicon ? "Apple Silicon" : aiCapabilities.hardware.architecture}</dd></div>
          <div><dt>Acceleration</dt><dd>{aiCapabilities.hardware.acceleration === "metal" ? "Apple Metal" : "CPU"}</dd></div>
          <div><dt>Memory</dt><dd>{formatGiB(aiCapabilities.hardware.physicalMemoryBytes)}</dd></div>
          <div><dt>Free storage</dt><dd>{formatGiB(aiCapabilities.hardware.availableDiskBytes)}</dd></div>
        </dl>
        <div className="model-card"><span><strong>{aiCapabilities.recommendation.artifact.displayName}</strong><small>{aiCapabilities.recommendation.reason}</small><small>Download {formatGiB(aiCapabilities.recommendation.estimatedDownloadBytes)} · safe staging requires {formatGiB(aiCapabilities.recommendation.requiredStorageBytes)} · {aiCapabilities.recommendation.artifact.license}</small></span>
          {aiCapabilities.lifecycle === "installed" ? <div className="runtime-actions"><button type="button" disabled={aiAction !== null} onClick={testAiRuntime}>{aiAction === "testing" ? "Testing local runtime…" : "Test private AI runtime"}</button><button type="button" disabled={aiAction !== null} onClick={testAiEvaluation}>{aiAction === "evaluating" ? "Running synthetic evaluation…" : "Run synthetic AI evaluation"}</button><button type="button" disabled={aiAction !== null} onClick={testPersistentAiEvaluation}>{aiAction === "persistentEvaluating" ? "Testing persistent worker…" : "Run persistent worker evaluation"}</button><button type="button" className="secondary" disabled={aiAction !== null} onClick={removeAiModel}>{aiAction === "removing" ? "Removing…" : "Remove model"}</button></div> : <button type="button" disabled={!aiCapabilities.recommendation.eligible || aiAction !== null} onClick={downloadAiModel}>{aiAction === "downloading" ? "Downloading private model…" : "Download private AI model"}</button>}
          {aiAction === "downloading" && aiProgress && <div className="download-progress"><progress max={aiProgress.totalBytes} value={aiProgress.downloadedBytes} /><small>{Math.round(aiProgress.downloadedBytes / aiProgress.totalBytes * 100)}% · {formatGiB(aiProgress.downloadedBytes)} of {formatGiB(aiProgress.totalBytes)}</small></div>}
          <small className="integrity">Pinned revision · SHA-256 verified before installation · inference remains disabled</small>
          {runtimeHealth && <output aria-live="polite">Runtime ready · {runtimeHealth.acceleration === "metal" ? "Apple Metal" : "CPU fallback"} · {runtimeHealth.runtimeVersion}</output>}
          {evaluation && <output aria-live="polite">Synthetic evaluation · {evaluation.passed} of {evaluation.total} passed{evaluation.allPassed ? " · release gate passed" : ` · release gate remains closed · failed: ${evaluation.cases.filter((result) => !result.passed).map((result) => `${result.id} [${result.failureCode ?? result.observedClassification ?? "rejected"}; tasks ${result.taskCount ?? "-"}, appointments ${result.appointmentCount ?? "-"}, waiting ${result.waitingCount ?? "-"}]`).join(", ")}`}</output>}
          {persistentEvaluation && <output aria-live="polite">Persistent worker · {persistentEvaluation.passed} of {persistentEvaluation.total} passed{persistentEvaluation.allPassed ? " · worker gate passed" : ` · worker gate remains closed · failed: ${persistentEvaluation.cases.filter((result) => !result.passed).map((result) => `${result.id} [${result.failureCode ?? "rejected"}; class ${result.observedClassification ?? "-"}, tasks ${result.taskCount ?? "-"}, appointments ${result.appointmentCount ?? "-"}, waiting ${result.waitingCount ?? "-"}]`).join(", ")}`}</output>}
        </div>
      </>}
      {aiError && <output aria-live="polite">{aiError}</output>}
    </section>}
    <aside><strong>Local-first by design</strong><p>Email content is not sent to an external AI service. Complete message bodies are not stored by default.</p></aside>
  </main>;
}

function formatGiB(bytes: number) { return `${(bytes / 1024 ** 3).toFixed(1)} GB`; }
function correspondenceOutcomeLabel(entry: CorrespondenceExecutionHistory) {
  if (entry.reasonCode === "provider_accepted") return "Microsoft accepted the reply";
  if (entry.reasonCode === "draft_missing") return "Private draft was unavailable; nothing was sent";
  if (entry.reasonCode === "draft_integrity_failed") return "Private draft integrity check failed; nothing was sent";
  if (entry.reasonCode === "transport_ambiguous") return "Delivery was uncertain; automatic retry was blocked";
  return titleCase(entry.reasonCode.replaceAll("_", " "));
}
function reviewKey(item: { accountId: string; providerId: string }) { return `${item.accountId}:${item.providerId}`; }
function reviewReasonLabel(reason: string) { return titleCase(reason.replace(/([a-z])([A-Z])/g, "$1 $2").replaceAll("_", " ")); }
function suggestionLabel(suggestion: ReviewSuggestion) {
  if (suggestion.kind === "task") return suggestion.due_at ? `${suggestion.title} · due ${suggestion.due_at}` : suggestion.title;
  if (suggestion.kind === "appointment") return suggestion.start_at ? `${suggestion.title} · ${suggestion.start_at}` : suggestion.title;
  const followUp = suggestion.follow_up_at ? ` · follow up ${suggestion.follow_up_at}` : "";
  return `${suggestion.description}${followUp}`;
}
function localItemDate(item: LocalItem) { return item.scheduledAt || item.dueAt || item.startAt || item.followUpAt; }
function localNowRfc3339() {
  const now = new Date();
  const offset = -now.getTimezoneOffset();
  const sign = offset >= 0 ? "+" : "-";
  const absolute = Math.abs(offset);
  return `${now.toISOString().slice(0, -1)}${sign}${String(Math.floor(absolute / 60)).padStart(2, "0")}:${String(absolute % 60).padStart(2, "0")}`;
}
function localDayBounds(): [string, string] { const start = new Date(); start.setHours(0, 0, 0, 0); const end = new Date(start); end.setDate(end.getDate() + 1); return [start.toISOString(), end.toISOString()]; }
function graphUtcDate(value: string) { return new Date(`${value.replace(/(\.\d{3})\d+$/, "$1")}Z`); }
function formatCalendarCandidate(value: string) { return graphUtcDate(value).toLocaleString([], { dateStyle: "medium", timeStyle: "short" }); }
function toLocalDateTimeInput(value: string, addMinutes = 0) { const date = graphUtcDate(value); date.setMinutes(date.getMinutes() + addMinutes); const offset = date.getTimezoneOffset() * 60_000; return new Date(date.getTime() - offset).toISOString().slice(0, 16); }
function titleCase(value: string) { return value.charAt(0).toUpperCase() + value.slice(1); }
function minuteToTime(value: number) { return `${String(Math.floor(value / 60)).padStart(2, "0")}:${String(value % 60).padStart(2, "0")}`; }
function timeToMinute(value: string) { const [hour, minute] = value.split(":").map(Number); return hour * 60 + minute; }
