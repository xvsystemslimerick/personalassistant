use thiserror::Error;

pub trait SecretStore: Send + Sync {
    fn put(&self, account_id: &str, secret: &str) -> Result<(), SecretError>;
    fn get(&self, account_id: &str) -> Result<Option<String>, SecretError>;
    fn delete(&self, account_id: &str) -> Result<(), SecretError>;
}

#[cfg(target_os = "macos")]
pub struct MacKeychain {
    service: String,
}

#[cfg(not(target_os = "macos"))]
pub struct MacKeychain;

#[cfg(target_os = "macos")]
impl MacKeychain {
    pub fn new(bundle_identifier: &str) -> Self {
        Self::named(bundle_identifier, "microsoft-oauth")
    }

    pub fn named(bundle_identifier: &str, purpose: &str) -> Self {
        Self {
            service: format!("{bundle_identifier}.{purpose}"),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl MacKeychain {
    pub fn new(_bundle_identifier: &str) -> Self {
        Self
    }

    pub fn named(_bundle_identifier: &str, _purpose: &str) -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
impl SecretStore for MacKeychain {
    fn put(&self, account_id: &str, secret: &str) -> Result<(), SecretError> {
        security_framework::passwords::set_generic_password(
            &self.service,
            account_id,
            secret.as_bytes(),
        )
        .map_err(|_| SecretError::Unavailable)
    }
    fn get(&self, account_id: &str) -> Result<Option<String>, SecretError> {
        match security_framework::passwords::get_generic_password(&self.service, account_id) {
            Ok(value) => String::from_utf8(value)
                .map(Some)
                .map_err(|_| SecretError::InvalidEncoding),
            Err(error) if error.code() == -25300 => Ok(None),
            Err(_) => Err(SecretError::Unavailable),
        }
    }
    fn delete(&self, account_id: &str) -> Result<(), SecretError> {
        match security_framework::passwords::delete_generic_password(&self.service, account_id) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == -25300 => Ok(()),
            Err(_) => Err(SecretError::Unavailable),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl SecretStore for MacKeychain {
    fn put(&self, _account_id: &str, _secret: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }

    fn get(&self, _account_id: &str) -> Result<Option<String>, SecretError> {
        Err(SecretError::Unavailable)
    }

    fn delete(&self, _account_id: &str) -> Result<(), SecretError> {
        Err(SecretError::Unavailable)
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn unsupported_platform_secret_store_fails_closed() {
        let store = MacKeychain::new("example.invalid");
        assert!(matches!(
            store.put("account", "secret"),
            Err(SecretError::Unavailable)
        ));
        assert!(matches!(
            store.get("account"),
            Err(SecretError::Unavailable)
        ));
        assert!(matches!(
            store.delete("account"),
            Err(SecretError::Unavailable)
        ));
    }
}

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("system credential store is unavailable")]
    Unavailable,
    #[error("credential was not valid UTF-8")]
    InvalidEncoding,
}
