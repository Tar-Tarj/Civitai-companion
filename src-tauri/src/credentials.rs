use keyring::{Entry, Error as KeyringError};
use zeroize::{Zeroize, Zeroizing};

use crate::error::{AppError, AppResult};

const CREDENTIAL_SERVICE: &str = "Civitai Companion Desktop";
const CREDENTIAL_ACCOUNT: &str = "civitai-api-key";
const MAX_CREDENTIAL_BYTES: usize = 4096;

fn entry() -> AppResult<Entry> {
    Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT).map_err(|_| AppError::Credential)
}

pub fn is_configured() -> AppResult<bool> {
    match entry()?.get_password() {
        Ok(mut value) => {
            let configured = !value.is_empty();
            value.zeroize();
            Ok(configured)
        }
        Err(KeyringError::NoEntry) => Ok(false),
        Err(_) => Err(AppError::Credential),
    }
}

pub fn read() -> AppResult<Zeroizing<String>> {
    match entry()?.get_password() {
        Ok(value) if !value.is_empty() => Ok(Zeroizing::new(value)),
        Ok(mut value) => {
            value.zeroize();
            Err(AppError::MissingCredential)
        }
        Err(KeyringError::NoEntry) => Err(AppError::MissingCredential),
        Err(_) => Err(AppError::Credential),
    }
}

pub fn store(value: &str) -> AppResult<()> {
    validate(value)?;
    entry()?
        .set_password(value.trim())
        .map_err(|_| AppError::Credential)
}

pub fn validate(value: &str) -> AppResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > MAX_CREDENTIAL_BYTES
        || trimmed.chars().any(char::is_control)
    {
        return Err(AppError::InvalidCredential);
    }
    Ok(())
}

pub fn remove() -> AppResult<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(_) => Err(AppError::Credential),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_constants_do_not_contain_a_secret() {
        assert_eq!(CREDENTIAL_ACCOUNT, "civitai-api-key");
        assert_eq!(MAX_CREDENTIAL_BYTES, 4096);
    }
}
