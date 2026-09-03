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

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("system credential store is unavailable")]
    Unavailable,
    #[error("credential was not valid UTF-8")]
    InvalidEncoding,
}
