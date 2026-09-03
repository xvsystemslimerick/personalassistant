use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;

use crate::MICROSOFT_AUTHORITY;

pub const REQUIRED_SCOPES: &[&str] = &[
    "openid",
    "profile",
    "offline_access",
    "User.Read",
    "Mail.Read",
    "Mail.Send",
    "Calendars.ReadWrite",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingAuthorization {
    pub authorization_url: Url,
    pub state: String,
    pub code_verifier: String,
}

pub fn begin_authorization(
    client_id: &str,
    redirect_uri: &Url,
) -> Result<PendingAuthorization, OAuthError> {
    validate_client_id(client_id)?;
    validate_redirect_uri(redirect_uri)?;
    let state = random_urlsafe(32);
    let code_verifier = random_urlsafe(64);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    let mut url = Url::parse(&format!("{MICROSOFT_AUTHORITY}authorize"))?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri.as_str())
        .append_pair("response_mode", "query")
        .append_pair("scope", &REQUIRED_SCOPES.join(" "))
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(PendingAuthorization {
        authorization_url: url,
        state,
        code_verifier,
    })
}

pub fn validate_callback(
    callback: &Url,
    expected_redirect: &Url,
    expected_state: &str,
) -> Result<String, OAuthError> {
    if callback.scheme() != expected_redirect.scheme()
        || callback.host_str() != expected_redirect.host_str()
        || callback.port_or_known_default() != expected_redirect.port_or_known_default()
        || callback.path() != expected_redirect.path()
    {
        return Err(OAuthError::InvalidCallback);
    }
    let values: std::collections::HashMap<_, _> = callback.query_pairs().into_owned().collect();
    if let Some(error) = values.get("error") {
        return Err(OAuthError::Provider(error.clone()));
    }
    if values.get("state").map(String::as_str) != Some(expected_state) {
        return Err(OAuthError::StateMismatch);
    }
    values
        .get("code")
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or(OAuthError::MissingCode)
}

pub async fn exchange_code(
    client_id: &str,
    redirect_uri: &Url,
    code: &str,
    verifier: &str,
) -> Result<TokenSet, OAuthError> {
    validate_client_id(client_id)?;
    validate_redirect_uri(redirect_uri)?;
    let response = reqwest::Client::builder()
        .https_only(true)
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .post(format!("{MICROSOFT_AUTHORITY}token"))
        .form(&[
            ("client_id", client_id),
            ("scope", &REQUIRED_SCOPES.join(" ")),
            ("code", code),
            ("redirect_uri", redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier),
        ])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(OAuthError::TokenExchange(response.status().as_u16()));
    }
    let tokens: TokenSet = response.json().await?;
    if tokens.access_token.is_empty()
        || tokens
            .refresh_token
            .as_deref()
            .unwrap_or_default()
            .is_empty()
    {
        return Err(OAuthError::MissingToken);
    }
    Ok(tokens)
}

pub async fn refresh_access_token(
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenSet, OAuthError> {
    validate_client_id(client_id)?;
    if refresh_token.is_empty() {
        return Err(OAuthError::MissingToken);
    }
    let scopes = REQUIRED_SCOPES.join(" ");
    let response = reqwest::Client::builder()
        .https_only(true)
        .timeout(std::time::Duration::from_secs(30))
        .build()?
        .post(format!("{MICROSOFT_AUTHORITY}token"))
        .form(&[
            ("client_id", client_id),
            ("scope", scopes.as_str()),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(OAuthError::TokenExchange(response.status().as_u16()));
    }
    let tokens: TokenSet = response.json().await?;
    if tokens.access_token.is_empty() {
        return Err(OAuthError::MissingToken);
    }
    Ok(tokens)
}

fn validate_redirect_uri(uri: &Url) -> Result<(), OAuthError> {
    if uri.scheme() != "http"
        || uri.host_str() != Some("localhost")
        || uri.port().is_none()
        || uri.path() != "/oauth/callback"
    {
        return Err(OAuthError::InvalidCallback);
    }
    Ok(())
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
    pub scope: Option<String>,
    pub token_type: String,
    pub id_token: Option<String>,
}

fn validate_client_id(client_id: &str) -> Result<(), OAuthError> {
    if client_id.len() != 36 || !client_id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return Err(OAuthError::InvalidClientId);
    }
    Ok(())
}

fn random_urlsafe(bytes: usize) -> String {
    let mut value = vec![0_u8; bytes];
    rand::rng().fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("Microsoft application client ID is not configured")]
    InvalidClientId,
    #[error("OAuth callback was not a localhost redirect")]
    InvalidCallback,
    #[error("OAuth callback state did not match")]
    StateMismatch,
    #[error("OAuth callback did not include an authorization code")]
    MissingCode,
    #[error("Microsoft sign-in returned: {0}")]
    Provider(String),
    #[error("Microsoft token exchange returned HTTP {0}")]
    TokenExchange(u16),
    #[error("Microsoft token response did not include required tokens")]
    MissingToken,
    #[error("OAuth transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("could not construct OAuth URL: {0}")]
    Url(#[from] url::ParseError),
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT_ID: &str = "00000000-0000-0000-0000-000000000001";

    #[test]
    fn authorization_uses_pkce_and_minimum_configured_scopes() {
        let redirect = Url::parse("http://localhost:49152/oauth/callback").unwrap();
        let pending = begin_authorization(CLIENT_ID, &redirect).unwrap();
        let query: std::collections::HashMap<_, _> = pending
            .authorization_url
            .query_pairs()
            .into_owned()
            .collect();
        assert_eq!(query.get("response_type").unwrap(), "code");
        assert_eq!(query.get("code_challenge_method").unwrap(), "S256");
        assert_eq!(query.get("redirect_uri").unwrap(), redirect.as_str());
        assert!(query.get("scope").unwrap().contains("Mail.Read"));
        assert!(query.get("scope").unwrap().contains("Mail.Send"));
        assert!(query.get("scope").unwrap().contains("Calendars.ReadWrite"));
        assert!(!query.get("scope").unwrap().contains("Calendars.Read "));
        assert!(!query.contains_key("client_secret"));
    }

    #[test]
    fn callback_requires_exact_state_and_localhost() {
        let redirect = Url::parse("http://localhost:49152/oauth/callback").unwrap();
        let good =
            Url::parse("http://localhost:49152/oauth/callback?code=abc&state=expected").unwrap();
        assert_eq!(
            validate_callback(&good, &redirect, "expected").unwrap(),
            "abc"
        );
        assert!(matches!(
            validate_callback(&good, &redirect, "wrong"),
            Err(OAuthError::StateMismatch)
        ));
        let hostile = Url::parse("https://example.com?code=abc&state=expected").unwrap();
        assert!(matches!(
            validate_callback(&hostile, &redirect, "expected"),
            Err(OAuthError::InvalidCallback)
        ));
    }
}
