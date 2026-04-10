use anyhow::Result;
use std::path::PathBuf;

/// プラットフォーム別設定ディレクトリを取得（存在しない場合は作成）
///
/// - Windows: `%APPDATA%/<app_name>`  (`dirs::config_dir`)
/// - macOS:   `~/Library/Application Support/<app_name>`  (`dirs::data_local_dir`)
/// - Linux:   `~/.config/<app_name>`  (`dirs::config_dir`)
pub fn get_config_dir(app_name: &str) -> Result<PathBuf> {
    let base_dir = if cfg!(target_os = "windows") {
        dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
    } else if cfg!(target_os = "macos") {
        // macOS では ~/Library/Application Support を使用する
        dirs::data_local_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
    } else {
        // Linux: ~/.config
        dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
    };

    let path = base_dir.join(app_name);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// 設定ファイルのフルパスを返す
pub fn get_config_file_path(app_name: &str, file_name: &str) -> Result<PathBuf> {
    let config_dir = get_config_dir(app_name)?;
    Ok(config_dir.join(file_name))
}
