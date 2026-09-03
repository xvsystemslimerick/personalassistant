use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;
use zeroize::Zeroize;

use crate::GRAPH_BASE_URL;

pub const MAX_PROVIDER_RESPONSE_BYTES: usize = 96 * 1024;
pub const MAX_EMAIL_BODY_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub struct GraphClient {
    http: reqwest::Client,
}

impl GraphClient {
    pub fn new() -> Result<Self, GraphError> {
        let http = reqwest::Client::builder()
            .https_only(true)
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("PersonalAssistant/0.1")
            .build()?;
        Ok(Self { http })
    }

    pub async fn get_page<T: for<'de> Deserialize<'de>>(
        &self,
        access_token: &str,
        url: &Url,
    ) -> Result<GraphPage<T>, GraphError> {
        validate_graph_url(url)?;
        let response = self.send_with_retry(access_token, url, true, false).await?;
        let page = response.json::<GraphPage<T>>().await?;
        if let Some(next) = &page.next_link {
            validate_graph_url(next)?;
        }
        if let Some(delta) = &page.delta_link {
            validate_graph_url(delta)?;
        }
        Ok(page)
    }

    pub async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        access_token: &str,
        url: &Url,
    ) -> Result<T, GraphError> {
        validate_graph_url(url)?;
        let response = self
            .send_with_retry(access_token, url, false, false)
            .await?;
        Ok(response.json().await?)
    }

    /// Retrieves one message body transiently for qualified local analysis.
    /// The caller owns the returned allocation and must not persist it unless a
    /// separate encrypted-storage policy explicitly permits that operation.
    pub async fn get_message_content(
        &self,
        access_token: &str,
        provider_id: &str,
    ) -> Result<PrivateMessageContent, GraphError> {
        let url = message_content_url(provider_id)?;
        let mut response = self
            .send_with_retry(access_token, &url, false, true)
            .await?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PROVIDER_RESPONSE_BYTES as u64)
        {
            return Err(GraphError::ResponseTooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len().saturating_add(chunk.len()) > MAX_PROVIDER_RESPONSE_BYTES {
                return Err(GraphError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        decode_message_content(provider_id, &bytes)
    }

    /// Creates one event only from a validated, explicitly confirmed local
    /// request. The stable transaction ID is sent to Graph for duplicate
    /// suppression. No caller-controlled URL is accepted.
    pub async fn create_calendar_event(
        &self,
        access_token: &str,
        request: CalendarCreateRequest,
    ) -> Result<CalendarEvent, GraphError> {
        let payload = validate_calendar_create(request)?;
        let url = calendar_create_url();
        validate_graph_url(&url)?;
        let response = self
            .http
            .post(url)
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(GraphError::HttpStatus(response.status().as_u16()));
        }
        Ok(response.json().await?)
    }

    pub async fn update_calendar_event(
        &self,
        access_token: &str,
        request: CalendarUpdateRequest,
    ) -> Result<CalendarEvent, GraphError> {
        let (url, etag, payload) = validate_calendar_update(request)?;
        let response = self
            .http
            .patch(url)
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .header("If-Match", etag)
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(GraphError::HttpStatus(response.status().as_u16()));
        }
        Ok(response.json().await?)
    }

    /// Sends a reply only to the participants of one provider message. This
    /// adapter accepts no recipient or caller-selected URL. Desktop execution
    /// must enforce confirmation, draft integrity, and terminal no-retry audit.
    pub async fn reply_to_message(
        &self,
        access_token: &str,
        request: ReplyToMessageRequest,
    ) -> Result<(), GraphError> {
        let (url, payload) = validate_reply_to_message(request)?;
        let response = self
            .http
            .post(url)
            .bearer_auth(access_token)
            .header("Accept", "application/json")
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(GraphError::HttpStatus(response.status().as_u16()));
        }
        Ok(())
    }

    async fn send_with_retry(
        &self,
        access_token: &str,
        url: &Url,
        paged: bool,
        prefer_text_body: bool,
    ) -> Result<reqwest::Response, GraphError> {
        for attempt in 0..4 {
            let mut request = self
                .http
                .get(url.clone())
                .bearer_auth(access_token)
                .header("Accept", "application/json");
            if paged {
                request = request.header("Prefer", "odata.maxpagesize=50");
            }
            if prefer_text_body {
                request = request.header("Prefer", "outlook.body-content-type=\"text\"");
            }
            let response = request.send().await?;
            let status = response.status();
            if status.is_success() {
                return Ok(response);
            }
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            if attempt < 3
                && (status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
            {
                tokio::time::sleep(retry_delay(attempt, retry_after)).await;
                continue;
            }
            return if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                Err(GraphError::Throttled(retry_after))
            } else {
                Err(GraphError::HttpStatus(status.as_u16()))
            };
        }
        unreachable!("bounded retry loop always returns")
    }
}

pub fn message_content_url(provider_id: &str) -> Result<Url, GraphError> {
    if provider_id.is_empty()
        || provider_id.len() > 512
        || provider_id.chars().any(char::is_control)
    {
        return Err(GraphError::InvalidMessageId);
    }
    let mut url = Url::parse(&format!("{GRAPH_BASE_URL}me/messages"))?;
    url.path_segments_mut()
        .map_err(|_| GraphError::UntrustedUrl)?
        .push(provider_id);
    url.query_pairs_mut()
        .append_pair("$select", "id,subject,body,receivedDateTime,sentDateTime");
    validate_graph_url(&url)?;
    Ok(url)
}

fn decode_message_content(
    expected_provider_id: &str,
    bytes: &[u8],
) -> Result<PrivateMessageContent, GraphError> {
    if bytes.len() > MAX_PROVIDER_RESPONSE_BYTES {
        return Err(GraphError::ResponseTooLarge);
    }
    let message: GraphMessageContent =
        serde_json::from_slice(bytes).map_err(|_| GraphError::InvalidMessageResponse)?;
    if message.id != expected_provider_id {
        return Err(GraphError::MessageIdMismatch);
    }
    if !message.body.content_type.eq_ignore_ascii_case("text") {
        return Err(GraphError::UnexpectedBodyType);
    }
    if message.body.content.len() > MAX_EMAIL_BODY_BYTES {
        return Err(GraphError::BodyTooLarge);
    }
    Ok(PrivateMessageContent {
        provider_id: message.id,
        subject: message.subject,
        body: message.body.content,
        received_date_time: message.received_date_time,
        sent_date_time: message.sent_date_time,
    })
}

fn retry_delay(attempt: usize, retry_after: Option<u64>) -> std::time::Duration {
    std::time::Duration::from_secs(retry_after.unwrap_or(1_u64 << attempt.min(4)).clamp(1, 30))
}

pub fn initial_mail_delta_url(folder: MailFolder) -> Url {
    let folder = match folder {
        MailFolder::Inbox => "inbox",
        MailFolder::SentItems => "sentitems",
    };
    Url::parse(&format!("{GRAPH_BASE_URL}me/mailFolders/{folder}/messages/delta?$select=id,conversationId,sender,subject,receivedDateTime,sentDateTime,webLink,isRead&$top=50")).expect("static Graph URL")
}

pub fn initial_calendar_url(start: &str, end: &str) -> Result<Url, GraphError> {
    let mut url = Url::parse(&format!("{GRAPH_BASE_URL}me/calendarView"))?;
    url.query_pairs_mut()
        .append_pair("startDateTime", start)
        .append_pair("endDateTime", end)
        .append_pair(
            "$select",
            "id,subject,start,end,isCancelled,webLink,lastModifiedDateTime",
        )
        .append_pair("$top", "50");
    Ok(url)
}

fn calendar_create_url() -> Url {
    Url::parse(&format!("{GRAPH_BASE_URL}me/events")).expect("static Graph URL")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarCreateRequest {
    pub subject: String,
    pub start_at: String,
    pub end_at: String,
    pub transaction_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CalendarCreatePayload {
    subject: String,
    start: GraphDateTime,
    end: GraphDateTime,
    transaction_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarUpdateRequest {
    pub provider_event_id: String,
    pub provider_etag: String,
    pub start_at: String,
    pub end_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CalendarUpdatePayload {
    start: GraphDateTime,
    end: GraphDateTime,
}

pub struct ReplyToMessageRequest {
    pub provider_message_id: String,
    pub comment: String,
}

impl std::fmt::Debug for ReplyToMessageRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReplyToMessageRequest")
            .field("provider_message_id_bytes", &self.provider_message_id.len())
            .field("comment_bytes", &self.comment.len())
            .finish()
    }
}

impl Drop for ReplyToMessageRequest {
    fn drop(&mut self) {
        self.comment.zeroize();
    }
}

#[derive(Serialize)]
struct ReplyToMessagePayload {
    comment: String,
}

impl Drop for ReplyToMessagePayload {
    fn drop(&mut self) {
        self.comment.zeroize();
    }
}

fn validate_reply_to_message(
    mut request: ReplyToMessageRequest,
) -> Result<(Url, ReplyToMessagePayload), GraphError> {
    let comment = request.comment.trim();
    if request.provider_message_id.is_empty()
        || request.provider_message_id.len() > 512
        || request.provider_message_id.chars().any(char::is_control)
        || comment.is_empty()
        || comment.len() > 16 * 1024
        || comment
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(GraphError::InvalidCorrespondenceWrite);
    }
    let mut url = Url::parse(&format!("{GRAPH_BASE_URL}me/messages"))?;
    url.path_segments_mut()
        .map_err(|_| GraphError::UntrustedUrl)?
        .push(&request.provider_message_id)
        .push("reply");
    validate_graph_url(&url)?;
    let payload = ReplyToMessagePayload {
        comment: comment.to_owned(),
    };
    request.comment.zeroize();
    Ok((url, payload))
}

fn validate_calendar_update(
    request: CalendarUpdateRequest,
) -> Result<(Url, String, CalendarUpdatePayload), GraphError> {
    if request.provider_event_id.is_empty()
        || request.provider_event_id.len() > 512
        || request.provider_event_id.chars().any(char::is_control)
        || request.provider_etag.is_empty()
        || request.provider_etag.len() > 512
        || request.provider_etag.chars().any(char::is_control)
    {
        return Err(GraphError::InvalidCalendarWrite);
    }
    let start = chrono::DateTime::parse_from_rfc3339(&request.start_at)
        .map_err(|_| GraphError::InvalidCalendarWrite)?
        .with_timezone(&chrono::Utc);
    let end = chrono::DateTime::parse_from_rfc3339(&request.end_at)
        .map_err(|_| GraphError::InvalidCalendarWrite)?
        .with_timezone(&chrono::Utc);
    if end <= start || end - start > chrono::Duration::days(7) {
        return Err(GraphError::InvalidCalendarWrite);
    }
    let mut url = Url::parse(&format!("{GRAPH_BASE_URL}me/events"))?;
    url.path_segments_mut()
        .map_err(|_| GraphError::UntrustedUrl)?
        .push(&request.provider_event_id);
    validate_graph_url(&url)?;
    let graph_time = |value: chrono::DateTime<chrono::Utc>| GraphDateTime {
        date_time: value.format("%Y-%m-%dT%H:%M:%S").to_string(),
        time_zone: "UTC".into(),
    };
    Ok((
        url,
        request.provider_etag,
        CalendarUpdatePayload {
            start: graph_time(start),
            end: graph_time(end),
        },
    ))
}

fn validate_calendar_create(
    request: CalendarCreateRequest,
) -> Result<CalendarCreatePayload, GraphError> {
    let subject = request.subject.trim();
    if subject.is_empty()
        || subject.len() > 200
        || subject.chars().any(char::is_control)
        || request.transaction_id.len() < 16
        || request.transaction_id.len() > 100
        || !request
            .transaction_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
    {
        return Err(GraphError::InvalidCalendarWrite);
    }
    let start = chrono::DateTime::parse_from_rfc3339(&request.start_at)
        .map_err(|_| GraphError::InvalidCalendarWrite)?
        .with_timezone(&chrono::Utc);
    let end = chrono::DateTime::parse_from_rfc3339(&request.end_at)
        .map_err(|_| GraphError::InvalidCalendarWrite)?
        .with_timezone(&chrono::Utc);
    if end <= start || end - start > chrono::Duration::days(7) {
        return Err(GraphError::InvalidCalendarWrite);
    }
    let graph_time = |value: chrono::DateTime<chrono::Utc>| GraphDateTime {
        date_time: value.format("%Y-%m-%dT%H:%M:%S").to_string(),
        time_zone: "UTC".into(),
    };
    Ok(CalendarCreatePayload {
        subject: subject.to_owned(),
        start: graph_time(start),
        end: graph_time(end),
        transaction_id: request.transaction_id,
    })
}

pub fn validate_graph_url(url: &Url) -> Result<(), GraphError> {
    if url.scheme() != "https"
        || url.host_str() != Some("graph.microsoft.com")
        || !url.path().starts_with("/v1.0/")
        || url.username() != ""
        || url.password().is_some()
    {
        return Err(GraphError::UntrustedUrl);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailFolder {
    Inbox,
    SentItems,
}

#[derive(Debug, Deserialize)]
pub struct GraphPage<T> {
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<Url>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<Url>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MessageMetadata {
    pub id: String,
    pub conversation_id: Option<String>,
    pub sender: Option<EmailRecipient>,
    pub subject: Option<String>,
    pub received_date_time: Option<String>,
    pub sent_date_time: Option<String>,
    pub web_link: Option<String>,
    #[serde(default)]
    pub is_read: bool,
    #[serde(rename = "@removed")]
    pub removed: Option<Removed>,
}

/// Sensitive transient provider content. `Debug` intentionally reveals only
/// bounded metadata and never formats the subject or body.
pub struct PrivateMessageContent {
    pub provider_id: String,
    pub subject: Option<String>,
    pub body: String,
    pub received_date_time: Option<String>,
    pub sent_date_time: Option<String>,
}

impl std::fmt::Debug for PrivateMessageContent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrivateMessageContent")
            .field("provider_id", &self.provider_id)
            .field("body_bytes", &self.body.len())
            .finish_non_exhaustive()
    }
}

impl Drop for PrivateMessageContent {
    fn drop(&mut self) {
        self.subject.zeroize();
        self.body.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphMessageContent {
    id: String,
    subject: Option<String>,
    body: GraphMessageBody,
    received_date_time: Option<String>,
    sent_date_time: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphMessageBody {
    content_type: String,
    content: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EmailRecipient {
    pub email_address: EmailAddress,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct EmailAddress {
    pub name: Option<String>,
    pub address: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    #[serde(rename = "@odata.etag")]
    pub etag: Option<String>,
    pub id: String,
    pub subject: Option<String>,
    pub start: GraphDateTime,
    pub end: GraphDateTime,
    #[serde(default)]
    pub is_cancelled: bool,
    pub web_link: Option<String>,
    pub last_modified_date_time: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GraphDateTime {
    pub date_time: String,
    pub time_zone: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Removed {
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: String,
    pub display_name: String,
    pub mail: Option<String>,
    pub user_principal_name: String,
}

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("Graph returned HTTP {0}")]
    HttpStatus(u16),
    #[error("Graph throttled the request")]
    Throttled(Option<u64>),
    #[error("Graph returned an untrusted continuation URL")]
    UntrustedUrl,
    #[error("Graph transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("provider message identifier is invalid")]
    InvalidMessageId,
    #[error("provider message response exceeded the local analysis limit")]
    ResponseTooLarge,
    #[error("provider message body exceeded the local analysis limit")]
    BodyTooLarge,
    #[error("provider did not return the requested plaintext body")]
    UnexpectedBodyType,
    #[error("provider returned a mismatched message identifier")]
    MessageIdMismatch,
    #[error("provider message response was invalid")]
    InvalidMessageResponse,
    #[error("calendar write request failed validation")]
    InvalidCalendarWrite,
    #[error("correspondence write request failed validation")]
    InvalidCorrespondenceWrite,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuation_urls_are_pinned_to_graph_v1() {
        assert!(validate_graph_url(
            &Url::parse("https://graph.microsoft.com/v1.0/me/messages?$skiptoken=x").unwrap()
        )
        .is_ok());
        assert!(
            validate_graph_url(&Url::parse("https://evil.example/v1.0/me/messages").unwrap())
                .is_err()
        );
        assert!(validate_graph_url(
            &Url::parse("http://graph.microsoft.com/v1.0/me/messages").unwrap()
        )
        .is_err());
    }
    #[test]
    fn mail_sync_requests_metadata_without_body() {
        let url = initial_mail_delta_url(MailFolder::Inbox);
        assert!(!url.as_str().contains("body"));
        assert!(url.as_str().contains("messages/delta"));
    }

    #[test]
    fn parses_message_delta_fixture_without_bodies() {
        let page: GraphPage<MessageMetadata> = serde_json::from_str(
            r#"{"value":[{"id":"m1","conversationId":"c1","subject":"School form","sender":{"emailAddress":{"name":"School","address":"office@example.test"}},"receivedDateTime":"2026-08-14T08:00:00Z","isRead":false},{"id":"m2","@removed":{"reason":"deleted"}}],"@odata.deltaLink":"https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=opaque"}"#,
        )
        .unwrap();
        assert_eq!(page.value.len(), 2);
        assert_eq!(page.value[0].subject.as_deref(), Some("School form"));
        assert!(page.value[1].removed.is_some());
        assert!(page.delta_link.is_some());
    }

    #[test]
    fn retry_delay_is_bounded_and_honors_short_server_delay() {
        assert_eq!(retry_delay(0, None), std::time::Duration::from_secs(1));
        assert_eq!(retry_delay(3, None), std::time::Duration::from_secs(8));
        assert_eq!(retry_delay(0, Some(7)), std::time::Duration::from_secs(7));
        assert_eq!(
            retry_delay(0, Some(600)),
            std::time::Duration::from_secs(30)
        );
    }

    #[test]
    fn transient_content_url_encodes_the_provider_id_and_selects_only_required_fields() {
        let url = message_content_url("A/B + private").unwrap();
        assert_eq!(url.host_str(), Some("graph.microsoft.com"));
        assert!(url.path().ends_with("/A%2FB%20+%20private"));
        assert!(url.query().unwrap().contains("%24select="));
        assert!(!url.query().unwrap().contains("uniqueBody"));
    }

    #[test]
    fn transient_plaintext_content_is_bounded_and_redacted_from_debug() {
        let content = decode_message_content(
            "m1",
            br#"{"id":"m1","subject":"Private subject","body":{"contentType":"text","content":"Private email body"},"receivedDateTime":"2026-08-21T08:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(content.body, "Private email body");
        let debug = format!("{content:?}");
        assert!(!debug.contains("Private subject"));
        assert!(!debug.contains("Private email body"));
    }

    #[test]
    fn transient_content_rejects_html_mismatches_and_oversized_data() {
        assert!(matches!(
            decode_message_content(
                "m1",
                br#"{"id":"m1","body":{"contentType":"html","content":"<b>private</b>"}}"#
            ),
            Err(GraphError::UnexpectedBodyType)
        ));
        assert!(matches!(
            decode_message_content(
                "expected",
                br#"{"id":"other","body":{"contentType":"text","content":"private"}}"#
            ),
            Err(GraphError::MessageIdMismatch)
        ));
        assert!(matches!(
            decode_message_content("m1", &vec![b'x'; MAX_PROVIDER_RESPONSE_BYTES + 1]),
            Err(GraphError::ResponseTooLarge)
        ));
    }

    #[test]
    fn calendar_create_payload_is_bounded_utc_and_idempotent() {
        let payload = validate_calendar_create(CalendarCreateRequest {
            subject: "  Confirmed meeting  ".into(),
            start_at: "2026-08-27T13:00:00+01:00".into(),
            end_at: "2026-08-27T14:00:00+01:00".into(),
            transaction_id: "calendar-create-00000001".into(),
        })
        .unwrap();
        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(
            calendar_create_url().as_str(),
            "https://graph.microsoft.com/v1.0/me/events"
        );
        assert_eq!(json["subject"], "Confirmed meeting");
        assert_eq!(json["start"]["dateTime"], "2026-08-27T12:00:00");
        assert_eq!(json["start"]["timeZone"], "UTC");
        assert_eq!(json["transactionId"], "calendar-create-00000001");
    }

    #[test]
    fn calendar_create_rejects_invalid_ranges_and_keys() {
        let invalid = |end_at: &str, transaction_id: &str| CalendarCreateRequest {
            subject: "Meeting".into(),
            start_at: "2026-08-27T13:00:00Z".into(),
            end_at: end_at.into(),
            transaction_id: transaction_id.into(),
        };
        assert!(matches!(
            validate_calendar_create(invalid("2026-08-27T12:00:00Z", "calendar-create-00000001")),
            Err(GraphError::InvalidCalendarWrite)
        ));
        assert!(matches!(
            validate_calendar_create(invalid("2026-08-27T14:00:00Z", "short")),
            Err(GraphError::InvalidCalendarWrite)
        ));
    }

    #[test]
    fn calendar_update_is_pinned_encoded_and_requires_etag() {
        let (url, etag, payload) = validate_calendar_update(CalendarUpdateRequest {
            provider_event_id: "event/id + opaque".into(),
            provider_etag: "W/\"version-1\"".into(),
            start_at: "2026-08-28T11:00:00+01:00".into(),
            end_at: "2026-08-28T11:15:00+01:00".into(),
        })
        .unwrap();
        assert_eq!(url.host_str(), Some("graph.microsoft.com"));
        assert!(url.path().ends_with("/event%2Fid%20+%20opaque"));
        assert_eq!(etag, "W/\"version-1\"");
        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(json["start"]["dateTime"], "2026-08-28T10:00:00");
        assert!(matches!(
            validate_calendar_update(CalendarUpdateRequest {
                provider_event_id: "event".into(),
                provider_etag: "".into(),
                start_at: "2026-08-28T11:00:00+01:00".into(),
                end_at: "2026-08-28T11:15:00+01:00".into(),
            }),
            Err(GraphError::InvalidCalendarWrite)
        ));
    }

    #[test]
    fn reply_is_source_bound_encoded_bounded_and_redacted() {
        let request = ReplyToMessageRequest {
            provider_message_id: "message/id + opaque".into(),
            comment: "  Thanks, I can confirm.\n  ".into(),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("Thanks"));
        let (url, payload) = validate_reply_to_message(request).unwrap();
        assert_eq!(url.host_str(), Some("graph.microsoft.com"));
        assert!(url.path().ends_with("/message%2Fid%20+%20opaque/reply"));
        let json = serde_json::to_value(payload).unwrap();
        assert_eq!(json["comment"], "Thanks, I can confirm.");
        for comment in ["", "\0hidden"] {
            assert!(matches!(
                validate_reply_to_message(ReplyToMessageRequest {
                    provider_message_id: "provider-message".into(),
                    comment: comment.into(),
                }),
                Err(GraphError::InvalidCorrespondenceWrite)
            ));
        }
        assert!(matches!(
            validate_reply_to_message(ReplyToMessageRequest {
                provider_message_id: "provider-message".into(),
                comment: "x".repeat(16 * 1024 + 1),
            }),
            Err(GraphError::InvalidCorrespondenceWrite)
        ));
    }
}
