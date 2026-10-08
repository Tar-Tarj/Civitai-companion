use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Credential operation failed")]
    Credential,
    #[error("The API key is required")]
    MissingCredential,
    #[error("The API key is invalid")]
    InvalidCredential,
    #[error("Local state could not be read")]
    StateRead,
    #[error("Local state could not be saved")]
    StateWrite,
    #[error("The stored data format is unsupported")]
    UnsupportedState,
    #[error("The settings backup is invalid")]
    InvalidBackup,
    #[error("The settings backup is too large")]
    BackupTooLarge,
    #[error("The requested value is invalid")]
    InvalidInput,
    #[error("The requested address is not allowed")]
    UnsafeUrl,
    #[error("Unable to reach Civitai")]
    Network,
    #[error("Civitai did not respond in time")]
    Timeout,
    #[error("Authentication expired or is invalid")]
    Unauthorized,
    #[error("The API key is missing a required scope")]
    Forbidden,
    #[error("Civitai rate limit reached; retry later")]
    RateLimited,
    #[error("Civitai is temporarily unavailable")]
    ServiceUnavailable,
    #[error("Civitai returned an unsupported response")]
    InvalidResponse,
    #[error("The operation failed")]
    Operation,
}

impl AppError {
    pub fn public_message(&self) -> String {
        self.to_string()
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_errors_are_fixed_and_never_echo_input() {
        let marker = "do-not-leak-this-credential";
        let errors = [
            AppError::Credential,
            AppError::InvalidCredential,
            AppError::Network,
            AppError::Unauthorized,
            AppError::InvalidResponse,
            AppError::Operation,
        ];
        for error in errors {
            let message = error.public_message();
            assert!(!message.contains(marker));
            assert!(message.len() < 128);
        }
    }
}
