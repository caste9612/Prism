//! Error types for the Prism application
//!
//! This module provides typed errors for better error handling and
//! more informative error messages.

use thiserror::Error;

/// Main error type for Prism operations
#[derive(Error, Debug)]
pub enum PrismError {
    /// Database-related errors
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// IO errors during file operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Errors during filesystem scanning
    #[error("Scan error: {0}")]
    Scan(String),

    /// Errors during duplicate detection
    #[error("Duplicate detection error: {0}")]
    Duplicate(String),

    /// Search-related errors
    #[error("Search error: {0}")]
    Search(String),

    /// Invalid path errors
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// File not found errors
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// Permission denied errors
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Serialization/deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Image processing errors
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    /// General errors with context
    #[error("{context}: {source}")]
    WithContext {
        context: String,
        #[source]
        source: Box<PrismError>,
    },
}

impl PrismError {
    /// Add context to an error
    pub fn with_context<C: Into<String>>(self, context: C) -> Self {
        PrismError::WithContext {
            context: context.into(),
            source: Box::new(self),
        }
    }

    /// Create a scan error with a message
    pub fn scan<S: Into<String>>(msg: S) -> Self {
        PrismError::Scan(msg.into())
    }

    /// Create a search error with a message
    pub fn search<S: Into<String>>(msg: S) -> Self {
        PrismError::Search(msg.into())
    }

    /// Create a duplicate detection error
    pub fn duplicate<S: Into<String>>(msg: S) -> Self {
        PrismError::Duplicate(msg.into())
    }

    /// Create a file not found error
    pub fn not_found<S: Into<String>>(path: S) -> Self {
        PrismError::FileNotFound(path.into())
    }

    /// Create a permission denied error
    pub fn permission_denied<S: Into<String>>(path: S) -> Self {
        PrismError::PermissionDenied(path.into())
    }
}

/// Result type alias for Prism operations
pub type PrismResult<T> = Result<T, PrismError>;

/// Conversion to String for Tauri commands
/// Tauri commands require Result<T, String>, so we provide this conversion
impl From<PrismError> for String {
    fn from(error: PrismError) -> Self {
        error.to_string()
    }
}

/// Convert anyhow errors to PrismError
impl From<anyhow::Error> for PrismError {
    fn from(error: anyhow::Error) -> Self {
        PrismError::Scan(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = PrismError::scan("Failed to read directory");
        assert_eq!(err.to_string(), "Scan error: Failed to read directory");
    }

    #[test]
    fn test_error_with_context() {
        let err = PrismError::scan("File access denied")
            .with_context("Processing C:\\Users");
        assert!(err.to_string().contains("Processing C:\\Users"));
    }

    #[test]
    fn test_error_conversion_to_string() {
        let err = PrismError::not_found("test.txt");
        let s: String = err.into();
        assert_eq!(s, "File not found: test.txt");
    }
}
