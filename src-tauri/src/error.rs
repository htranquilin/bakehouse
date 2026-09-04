use serde::Serialize;

/// App-wide error, serialized to the frontend as `{ code, message, detail? }`.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{message}")]
    Runtime { message: String, detail: Option<String> },
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub fn runtime(message: impl Into<String>, detail: Option<String>) -> Self {
        Self::Runtime { message: message.into(), detail }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Runtime { .. } => "runtime",
            Self::Internal(_) => "internal",
        }
    }
}

#[derive(Serialize)]
struct ErrorPayload<'a> {
    code: &'a str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<&'a str>,
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let detail = match self {
            Self::Runtime { detail, .. } => detail.as_deref(),
            _ => None,
        };
        ErrorPayload { code: self.code(), message: self.to_string(), detail }.serialize(serializer)
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
