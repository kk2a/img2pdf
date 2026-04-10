use thiserror::Error;

/// アプリケーション全体のエラー型
#[derive(Error, Debug)]
pub enum AppError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON Error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Image Error: {0}")]
    Image(String),

    #[error("PDF Error: {0}")]
    Pdf(String),

    #[error("Configuration Error: {0}")]
    Config(String),

    #[error("Validation Error: {0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, AppError>;
