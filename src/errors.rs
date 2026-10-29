/// A failure meant for the user: printed as-is, exits with `code`.
/// An empty message prints nothing (Ctrl+C).
#[derive(Debug)]
pub struct CliError {
    pub message: String,
    pub code: u8,
}

pub type Result<T> = std::result::Result<T, CliError>;

impl CliError {
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_code(message, 1)
    }

    pub fn with_code(message: impl Into<String>, code: u8) -> Self {
        Self {
            message: message.into(),
            code,
        }
    }
}

/// Bad command line: printed followed by the usage text, exits 2.
#[derive(Debug)]
pub struct UsageError(pub String);

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::new(e.to_string())
    }
}

impl From<inquire::InquireError> for CliError {
    fn from(e: inquire::InquireError) -> Self {
        match e {
            inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
                Self::with_code("", 130)
            }
            other => Self::new(other.to_string()),
        }
    }
}
