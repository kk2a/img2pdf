pub mod config;
pub mod error;

pub use config::Config;
pub use error::{AppError, Result};

/// 画像処理エラー情報
#[derive(Clone, Debug)]
pub struct ProcessingError {
    pub file_path: String,
    pub message: String,
}

/// 処理完了結果
#[derive(Clone, Debug)]
pub struct ProcessingResult {
    pub success: bool,
    pub success_count: usize,
    pub error_count: usize,
    pub errors: Vec<ProcessingError>,
    pub output_path: String,
}

/// 進捗フェーズ
#[derive(Debug, Clone)]
pub enum ProgressPhase {
    Processing,
    Saving,
}

/// 進捗更新情報
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub count: usize,
    pub total: usize,
    pub phase: ProgressPhase,
}
