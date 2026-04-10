# Rust 実装用 Cargo.toml テンプレート

以下のいずれかのGUIフレームワークを選択して使用してください。

## 推奨設定: FLTK-rs 版

```toml
[package]
name = "image_to_pdf"
version = "0.1.0"
edition = "2021"

[dependencies]
# 画像処理
image = "0.24"
imageproc = "0.23"

# PDF生成 (Option: pillow互換性が高め)
printpdf = "0.7"
# または
# pdfium-render = "0.8"

# GUI
fltk-rs = { version = "1.4", features = ["bundled"] }

# JSON設定
serde_json = "1.0"
serde = { version = "1.0", features = ["derive"] }

# パス/ファイルシステム
dirs = "5.0"  # クロスプラットフォーム ディレクトリ

# 並列処理
rayon = "1.7"

# エラーハンドリング
anyhow = "1.0"
thiserror = "1.0"

# ロギング
tracing = "0.1"
tracing-subscriber = "0.3"

# 非同期処理（必要に応じて）
tokio = { version = "1.35", features = ["full"] }

# EXIF処理
image-exif = "0.1"

[[bin]]
name = "image_to_pdf"
path = "src/main.rs"

[profile.release]
opt-level = 3
lto = true
codegen-units = 1
```

## 代替案: Iced 版 (モダンGUI)

```toml
[package]
name = "image_to_pdf"
version = "0.1.0"
edition = "2021"

[dependencies]
image = "0.24"
printpdf = "0.7"
iced = { version = "0.12", features = ["all"] }
serde_json = "1.0"
serde = { version = "1.0", features = ["derive"] }
dirs = "5.0"
rayon = "1.7"
anyhow = "1.0"
tokio = { version = "1.35", features = ["full"] }

[profile.release]
opt-level = 3
lto = true
```

## 代替案: GTK-rs 版 (Unix/Linux対応)

```toml
[package]
name = "image_to_pdf"
version = "0.1.0"
edition = "2021"

[dependencies]
image = "0.24"
printpdf = "0.7"
gtk = "0.17"
gdk = "0.17"
serde_json = "1.0"
serde = { version = "1.0", features = ["derive"] }
dirs = "5.0"
rayon = "1.7"
anyhow = "1.0"

[build-dependencies]
pkg-config = "0.3"

[profile.release]
opt-level = 3
lto = true
```

---

# プロジェクト構成

```
image-to-pdf/
├── Cargo.toml                   # 依存管理
├── Cargo.lock                   # ロック
├── src/
│   ├── main.rs                  # エントリーポイント
│   ├── app_config.rs            # AppConfig の Rust 実装
│   ├── image_processor.rs       # ImageProcessor の Rust 実装
│   ├── gui/
│   │   ├── mod.rs              # GUI モジュール
│   │   ├── fltk_app.rs         # FLTK UI実装
│   │   └── callbacks.rs        # UI コールバック
│   ├── models/
│   │   ├── mod.rs
│   │   ├── config.rs           # Config 構造体
│   │   └── error.rs            # エラー型定義
│   └── utils/
│       ├── mod.rs
│       ├── paths.rs            # クロスプラットフォーム パス処理
│       └── constants.rs        # 定数定義
├── tests/
│   ├── integration_tests.rs
│   └── image_processor_tests.rs
└── README.md
```

---

# src/main.rs サンプル構造

```rust
mod app_config;
mod image_processor;
mod gui;
mod models;
mod utils;

use std::sync::{Arc, Mutex};
use anyhow::Result;

const APP_NAME: &str = "ImageToPdf";

#[tokio::main]
async fn main() -> Result<()> {
    // ロギング初期化
    tracing_subscriber::fmt::init();

    // 設定読み込み
    let config = app_config::AppConfig::load()?;
    let config = Arc::new(Mutex::new(config));

    // GUI起動 (FLTK の場合)
    let app = fltk::app::App::default();
    gui::fltk_app::run(config.clone())?;

    Ok(())
}
```

---

# src/models/mod.rs

```rust
pub mod config;
pub mod error;

pub use config::Config;
pub use error::{AppError, Result};

#[derive(Clone, Debug)]
pub struct ProcessingResult {
    pub success: bool,
    pub success_count: usize,
    pub error_count: usize,
    pub errors: Vec<ProcessingError>,
    pub output_path: String,
}

#[derive(Clone, Debug)]
pub struct ProcessingError {
    pub file_path: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum ProgressPhase {
    Processing,
    Saving,
}

#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub count: usize,
    pub total: usize,
    pub phase: ProgressPhase,
}
```

---

# src/models/config.rs

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub last_save_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            last_save_dir: String::new(),
        }
    }
}
```

---

# src/models/error.rs

```rust
use thiserror::Error;

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
```

---

# src/utils/constants.rs

```rust
pub const APP_NAME: &str = "ImageToPdf";
pub const CONFIG_DIR_NAME: &str = "ImageToPdf";
pub const CONFIG_FILE_NAME: &str = "config.json";

pub const DEFAULT_WIDTH: u32 = 1654;
pub const A4_RATIO: f32 = 1.41421356;

pub const PDF_RESOLUTION: f32 = 72.0;
pub const PDF_QUALITY: u8 = 80;

pub const CANVAS_COLOR: (u8, u8, u8) = (255, 255, 255);
pub const SUPPORTED_FORMATS: &[&str] = &["jpg", "jpeg"];

pub const WINDOW_WIDTH: i32 = 600;
pub const WINDOW_HEIGHT: i32 = 500;

pub const MAX_WORKERS: usize = {
    let cpu_count = num_cpus::get();
    if cpu_count > 1 { cpu_count / 2 } else { 4 }
};
```

---

# src/utils/paths.rs

```rust
use std::path::PathBuf;
use anyhow::Result;

/// プラットフォーム別設定ディレクトリを取得
pub fn get_config_dir(app_name: &str) -> Result<PathBuf> {
    let config_dir = if cfg!(target_os = "windows") {
        // Windows: %APPDATA%
        dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
    } else if cfg!(target_os = "macos") {
        // macOS: ~/Library/Application Support
        dirs::data_local_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
    } else {
        // Linux: ~/.config
        dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
    };

    let path = config_dir.join(app_name);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// 設定ファイルパスを取得
pub fn get_config_file_path(app_name: &str, file_name: &str) -> Result<PathBuf> {
    let config_dir = get_config_dir(app_name)?;
    Ok(config_dir.join(file_name))
}
```

---

# src/app_config.rs スケルトン

```rust
use crate::models::Config;
use crate::utils::paths;
use anyhow::Result;
use std::path::PathBuf;

pub struct AppConfig {
    config_path: PathBuf,
    data: Config,
}

impl AppConfig {
    /// 設定を読み込む
    pub fn load() -> Result<Self> {
        let config_path = paths::get_config_file_path("ImageToPdf", "config.json")?;
        
        let data = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Config::default()
        };

        Ok(AppConfig { config_path, data })
    }

    /// 設定を保存
    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string(&self.data)?;
        std::fs::write(&self.config_path, json)?;
        Ok(())
    }

    pub fn get_last_save_dir(&self) -> &str {
        &self.data.last_save_dir
    }

    pub fn set_last_save_dir(&mut self, dir: String) {
        self.data.last_save_dir = dir;
    }
}
```

---

# src/image_processor.rs スケルトン

```rust
use crate::models::{ProcessingResult, ProcessingError, ProgressUpdate, ProgressPhase};
use crate::utils::constants::*;
use image::{ImageBuffer, Rgb, RgbImage};
use image::io::Reader as ImageReader;
use anyhow::Result;
use std::path::Path;
use rayon::prelude::*;
use std::sync::mpsc;

pub struct ImageProcessor {
    // コールバック用送信チャネル
    progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
    finished_tx: Option<mpsc::Sender<ProcessingResult>>,
}

impl ImageProcessor {
    pub fn new() -> Self {
        ImageProcessor {
            progress_tx: None,
            finished_tx: None,
        }
    }

    /// キャンバス高さを計算
    pub fn calculate_height(width: u32) -> u32 {
        ((width as f32) * A4_RATIO).round() as u32
    }

    /// 単一画像を処理
    pub fn process_single_image(
        file_path: &Path,
        canvas_w: u32,
        canvas_h: u32,
    ) -> std::result::Result<RgbImage, ProcessingError> {
        // 1. 画像読み込み
        let mut img = ImageReader::open(file_path)
            .and_then(|r| r.decode())
            .map_err(|e| ProcessingError {
                file_path: file_path.to_string_lossy().to_string(),
                message: format!("Failed to load image: {}", e),
            })?;

        // 2. EXIF回転処理 (実装例)
        // image-exif クレート使用
        // エラー時は無視

        // 3. RGB色空間に変換
        let img = img.to_rgb8();

        // 4. アスペクト比保持でリサイズ
        let (orig_w, orig_h) = (img.width(), img.height());
        let scale = f32::min(
            canvas_w as f32 / orig_w as f32,
            canvas_h as f32 / orig_h as f32,
        );
        let new_w = ((orig_w as f32) * scale).round() as u32;
        let new_h = ((orig_h as f32) * scale).round() as u32;

        let resized = image::imageops::resize(&img, new_w, new_h, image::imageops::FilterType::Lanczos3);

        // 5. キャンバスに中央配置
        let mut canvas: RgbImage = ImageBuffer::new(canvas_w, canvas_h);
        
        // 背景色で埋める
        for pixel in canvas.pixels_mut() {
            *pixel = Rgb(CANVAS_COLOR);
        }

        let offset_x = ((canvas_w - new_w) / 2) as i32;
        let offset_y = ((canvas_h - new_h) / 2) as i32;

        image::imageops::overlay(&mut canvas, &resized, offset_x as i64, offset_y as i64);

        Ok(canvas)
    }

    /// 複数画像を並列処理
    pub fn process_images(
        file_list: Vec<String>,
        canvas_width: u32,
    ) -> ProcessingResult {
        let canvas_height = Self::calculate_height(canvas_width);
        
        let results: Vec<_> = file_list
            .par_iter()
            .map(|f| {
                let path = Path::new(f);
                match Self::process_single_image(path, canvas_width, canvas_height) {
                    Ok(img) => (Some(img), None),
                    Err(e) => (None, Some(e)),
                }
            })
            .collect();

        let success_images: Vec<_> = results
            .iter()
            .filter_map(|(img, _)| img.clone())
            .collect();

        let errors: Vec<_> = results
            .iter()
            .filter_map(|(_, err)| err.clone())
            .collect();

        let success = !success_images.is_empty();
        let success_count = success_images.len();
        let error_count = errors.len();

        ProcessingResult {
            success,
            success_count,
            error_count,
            errors,
            output_path: String::new(), // 後で設定
        }
    }
}
```

---

# Cargo.toml に追加すべき依存

```toml
# 画像処理の追加ツール
num_threads = "0.1"
num_cpus = "1.16"

# ファイル操作の拡張
walkdir = "2.4"

# 進捗表示（CLI希望の場合）
indicatif = "0.17"

# クロスプラットフォームパス
# (既に dirs で対応)
```

---

# ビルドとテスト

```bash
# テスト
cargo test

# デバッグビルド
cargo build

# リリースビルド（最適化）
cargo build --release

# 実行
cargo run

# ドキュメント生成
cargo doc --open
```

---

# 依存のバージョン確認

```bash
cargo tree
cargo outdated
```

---

# クロスコンパイル例（Windows → Linux など）

```bash
# Windows → Linux
rustup target add x86_64-unknown-linux-gnu
cargo build --target x86_64-unknown-linux-gnu --release

# macOS → Windows
rustup target add x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu --release
```
