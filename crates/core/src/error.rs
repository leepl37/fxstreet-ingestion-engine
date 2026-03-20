use std::fmt;

#[derive(Debug)]
pub enum CoreError {
    Config(String),
    QuestDb(String),
    Http(reqwest::Error),
    Validation(String),
    Serialization(serde_json::Error),
    ExternalApi(String),
    ExternalApiStatus { status: u16, message: String },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Config(msg) => write!(f, "Configuration error: {}", msg),
            CoreError::QuestDb(msg) => write!(f, "QuestDB operation failed: {}", msg),
            CoreError::Http(err) => write!(f, "HTTP client error: {}", err),
            CoreError::Validation(msg) => write!(f, "Validation error: {}", msg),
            CoreError::Serialization(err) => write!(f, "Serialization error: {}", err),
            CoreError::ExternalApi(msg) => write!(f, "External API error: {}", msg),
            CoreError::ExternalApiStatus { status, message } => {
                write!(f, "External API status {}: {}", status, message)
            }
        }
    }
}

impl std::error::Error for CoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CoreError::Http(err) => Some(err),
            CoreError::Serialization(err) => Some(err),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for CoreError {
    fn from(err: reqwest::Error) -> Self {
        CoreError::Http(err)
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(err: serde_json::Error) -> Self {
        CoreError::Serialization(err)
    }
}

impl CoreError {
    pub fn is_retryable(&self) -> bool {
        match self {
            // transport-level failures are usually transient
            CoreError::Http(_) => true,
            // explicit status-based policy
            CoreError::ExternalApiStatus { status, .. } => *status == 429 || (500..=599).contains(status),
            _ => false,
        }
    }
}
