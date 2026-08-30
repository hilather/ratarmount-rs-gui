use std::fmt;

pub type Result<T> = std::result::Result<T, ApiError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorCode {
    NotFound,
    NotWritable,
    SiblingNotWritable,
    BadPassword,
    UnsupportedFormat,
    CorruptIndex,
    Cancelled,
    PreviewTooLarge,
    PathEscape,
    Busy,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "NotFound",
            Self::NotWritable => "NotWritable",
            Self::SiblingNotWritable => "SiblingNotWritable",
            Self::BadPassword => "BadPassword",
            Self::UnsupportedFormat => "UnsupportedFormat",
            Self::CorruptIndex => "CorruptIndex",
            Self::Cancelled => "Cancelled",
            Self::PreviewTooLarge => "PreviewTooLarge",
            Self::PathEscape => "PathEscape",
            Self::Busy => "Busy",
            Self::Internal => "Internal",
        }
    }

    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::Busy | Self::NotWritable | Self::SiblingNotWritable
        )
    }
}

#[derive(Clone, Debug)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn retryable(&self) -> bool {
        self.code.retryable()
    }

    /// JS command-error payload (`jobFailed` minus `jobId`).
    pub fn to_command_error(&self) -> CommandError {
        CommandError {
            code: self.code.as_str().to_string(),
            message: self.message.clone(),
            retryable: self.retryable(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn path_escape(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::PathEscape, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn busy(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Busy, message)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} (retryable={})",
            self.code.as_str(),
            self.message,
            self.retryable()
        )
    }
}

impl std::error::Error for ApiError {}

/// Thrown to JS as Error fields `{ code, message, retryable }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}
