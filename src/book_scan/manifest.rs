use super::config::BookScanConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookScanStage {
    Extracted,
    ScanTailored,
    Upscaled,
    Encoded,
    PdfWritten,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BlankPageMetrics {
    pub background_level: u8,
    pub dark_ratio: f32,
    pub edge_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRecord {
    pub index: usize,
    pub page_number: usize,
    pub stem: String,
    pub source_path: PathBuf,
    pub source_width: u32,
    pub source_height: u32,
    #[serde(default)]
    pub is_blank: bool,
    #[serde(default)]
    pub blank_metrics: Option<BlankPageMetrics>,
    pub crop_path: Option<PathBuf>,
    pub crop_width: u32,
    pub crop_height: u32,
    pub restore_x: f32,
    pub restore_y: f32,
    pub processed_path: Option<PathBuf>,
    pub jpeg_path: Option<PathBuf>,
    pub stage: BookScanStage,
    pub attempts: u8,
    pub error: Option<String>,
}

impl PageRecord {
    pub fn crop_pixels(&self) -> u64 {
        u64::from(self.crop_width) * u64::from(self.crop_height)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookScanManifest {
    pub schema_version: u32,
    pub config: BookScanConfig,
    pub pages: Vec<PageRecord>,
    pub completed: bool,
}

impl BookScanManifest {
    pub const SCHEMA_VERSION: u32 = 3;

    pub fn new(config: BookScanConfig, pages: Vec<PageRecord>) -> Self {
        Self {
            schema_version: Self::SCHEMA_VERSION,
            config,
            pages,
            completed: false,
        }
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| format!("manifestを読めません: {e}"))?;
        let manifest: Self =
            serde_json::from_slice(&bytes).map_err(|e| format!("manifestが不正です: {e}"))?;
        if manifest.schema_version != Self::SCHEMA_VERSION {
            return Err(format!(
                "未対応のmanifest schemaです: {}",
                manifest.schema_version
            ));
        }
        Ok(manifest)
    }

    pub fn save_atomic(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("manifestフォルダを作成できません: {e}"))?;
        }
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("manifestをJSON化できません: {e}"))?;
        fs::write(&tmp, bytes).map_err(|e| format!("manifest一時保存に失敗しました: {e}"))?;
        if cfg!(windows) && path.exists() {
            fs::remove_file(path).map_err(|e| format!("旧manifestを置換できません: {e}"))?;
        }
        fs::rename(&tmp, path).map_err(|e| format!("manifest確定に失敗しました: {e}"))?;
        Ok(())
    }
}
