use serde::{Deserialize, Serialize};

use crate::book_scan::BookScanConfig;

/// ユーザー設定を保持する構造体
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// 最後に使用した入力ディレクトリ
    #[serde(default)]
    pub last_input_dir: String,

    /// 最後に使用した保存ディレクトリ
    #[serde(default)]
    pub last_save_dir: String,

    /// 本モードで最後に使用した実行時設定。
    #[serde(default)]
    pub book_scan: Option<BookScanConfig>,
}
