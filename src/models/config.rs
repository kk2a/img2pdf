use serde::{Deserialize, Serialize};

/// ユーザー設定を保持する構造体
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// 最後に使用した入力ディレクトリ
    #[serde(default)]
    pub last_input_dir: String,

    /// 最後に使用した保存ディレクトリ
    #[serde(default)]
    pub last_save_dir: String,
}
