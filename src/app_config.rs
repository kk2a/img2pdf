use crate::models::Config;
use crate::utils::paths;
use anyhow::Result;
use std::path::PathBuf;

/// アプリケーション設定を管理する構造体
pub struct AppConfig {
    config_path: PathBuf,
    data: Config,
}

impl AppConfig {
    /// 設定を読み込んで新しい AppConfig を生成する
    pub fn new() -> Result<Self> {
        let config_path = paths::get_config_file_path("ImageToPdf", "config.json")?;

        let data = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Config::default()
        };

        Ok(AppConfig { config_path, data })
    }

    /// 設定を JSON ファイルに保存する（失敗は無視）
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.data) {
            let _ = std::fs::write(&self.config_path, json);
        }
    }

    /// 最後に使用した保存ディレクトリを返す
    pub fn get_last_save_dir(&self) -> &str {
        &self.data.last_save_dir
    }

    /// 最後に使用した保存ディレクトリを更新する
    pub fn set_last_save_dir(&mut self, dir: String) {
        self.data.last_save_dir = dir;
    }
}
