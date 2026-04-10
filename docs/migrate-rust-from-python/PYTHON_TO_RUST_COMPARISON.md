# Python → Rust コード対比ガイド

このドキュメントは、Python コードの各セクションが Rust でどのように実装されるべきかを示します。

---

## 1. インポートと定数

### Python
```python
import sys
import os
import json
import threading
import concurrent.futures
import math
import time
from pathlib import Path
import tkinter as tk
from tkinter import ttk, filedialog, messagebox
from PIL import Image, ImageOps
import cv2
import numpy as np

APP_NAME = "ImageToPdf"
CONFIG_DIR_NAME = "ImageToPdf"
CONFIG_FILE_NAME = "config.json"
DEFAULT_WIDTH = 1654
A4_RATIO = 1.41421356
```

### Rust 対応
```rust
// Cargo.toml 依存
// [dependencies]
// image = "0.24"
// serde_json = "1.0"
// rayon = "1.7"
// fltk-rs = "1.4"
// dirs = "5.0"

use std::path::PathBuf;
use serde_json::json;
use rayon::prelude::*;
use image::{ImageBuffer, Rgb};

// src/utils/constants.rs
pub const APP_NAME: &str = "ImageToPdf";
pub const CONFIG_DIR_NAME: &str = "ImageToPdf";
pub const CONFIG_FILE_NAME: &str = "config.json";
pub const DEFAULT_WIDTH: u32 = 1654;
pub const A4_RATIO: f32 = 1.41421356;
```

---

## 2. AppConfig クラス

### Python
```python
class AppConfig:
    def __init__(self):
        self.config_dir = self._get_config_dir()
        self.config_path = self.config_dir / CONFIG_FILE_NAME
        self.last_save_dir = ""
        self.load()

    def _get_config_dir(self):
        if sys.platform == "win32":
            base = os.environ.get("APPDATA")
        elif sys.platform == "darwin":
            base = os.path.expanduser("~/Library/Application Support")
        else:
            base = os.path.expanduser("~/.config")
        
        path = Path(base) / CONFIG_DIR_NAME
        if not path.exists():
            try:
                path.mkdir(parents=True, exist_ok=True)
            except OSError:
                pass
        return path

    def load(self):
        if self.config_path.exists():
            try:
                with open(self.config_path, "r", encoding="utf-8") as f:
                    data = json.load(f)
                    self.last_save_dir = data.get("last_save_dir", "")
            except Exception:
                pass

    def save(self):
        data = {
            "last_save_dir": self.last_save_dir
        }
        try:
            with open(self.config_path, "w", encoding="utf-8") as f:
                json.dump(data, f)
        except Exception:
            pass
```

### Rust 対応
```rust
// src/app_config.rs

use crate::models::Config;
use std::path::PathBuf;
use anyhow::Result;

pub struct AppConfig {
    config_path: PathBuf,
    data: Config,
}

impl AppConfig {
    pub fn new() -> Result<Self> {
        let config_dir = Self::get_config_dir()?;
        let config_path = config_dir.join("config.json");
        
        let data = if config_path.exists() {
            Self::load_json(&config_path).unwrap_or_default()
        } else {
            Config::default()
        };

        Ok(AppConfig { config_path, data })
    }

    fn get_config_dir() -> Result<PathBuf> {
        // クロスプラットフォーム対応
        let base_dir = if cfg!(target_os = "windows") {
            dirs::config_dir() // または std::env::var("APPDATA")
        } else if cfg!(target_os = "macos") {
            dirs::data_local_dir()
        } else {
            dirs::config_dir()
        };

        let config_dir = base_dir
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?
            .join("ImageToPdf");

        std::fs::create_dir_all(&config_dir)?;
        Ok(config_dir)
    }

    fn load_json(path: &PathBuf) -> Result<Config> {
        let content = std::fs::read_to_string(path)?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn load(&mut self) -> Result<()> {
        if self.config_path.exists() {
            self.data = Self::load_json(&self.config_path).unwrap_or_default();
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.data)?;
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

// src/models/config.rs (serde使用)
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub last_save_dir: String,
}
```

### ポイント
- `sys.platform` チェック → `cfg!(target_os = "...")` マクロ
- `Path` 操作 → `std::path::PathBuf`
- 例外処理 → `Result<T>` で返値化
- JSON読み書き → `serde_json`

---

## 3. ImageProcessor クラス - 初期化

### Python
```python
class ImageProcessor:
    def __init__(self, callback_progress, callback_finished):
        self.callback_progress = callback_progress
        self.callback_finished = callback_finished
        self.cancel_event = threading.Event()

    def calculate_height(self, width):
        return int(width * A4_RATIO + 0.5)
```

### Rust 対応
```rust
// src/image_processor.rs

use std::sync::mpsc;
use crate::models::{ProgressUpdate, ProcessingResult};

pub struct ImageProcessor {
    progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
    finished_tx: Option<mpsc::Sender<ProcessingResult>>,
    // キャンセル用 flag (必要な場合)
    // cancel_flag: Arc<AtomicBool>,
}

impl ImageProcessor {
    pub fn new(
        progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
        finished_tx: Option<mpsc::Sender<ProcessingResult>>,
    ) -> Self {
        ImageProcessor {
            progress_tx,
            finished_tx,
            // cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn calculate_height(width: u32) -> u32 {
        ((width as f32 * A4_RATIO) + 0.5) as u32
    }
}
```

### ポイント
- コールバック関数 → `mpsc::Sender` チャネル（スレッド間通信）
- `threading.Event()` → `Arc<AtomicBool>` または `parking_lot::Mutex`
- 計算式は同じだが型が異なる（u32 vs int）

---

## 4. ImageProcessor - 単一画像処理

### Python
```python
def process_single_image(self, file_path, canvas_w, canvas_h):
    try:
        # Load
        img = Image.open(file_path)
        
        # Exif Rotation
        try:
            img = ImageOps.exif_transpose(img)
        except Exception:
            pass

        # Convert to RGB
        if img.mode != 'RGB':
            img = img.convert('RGB')

        # Scale
        orig_w, orig_h = img.size
        scale = min(canvas_w / orig_w, canvas_h / orig_h)
        new_w = int(orig_w * scale + 0.5)
        new_h = int(orig_h * scale + 0.5)
        img_resized = img.resize((new_w, new_h))

        # Center on Canvas
        canvas = Image.new('RGB', (canvas_w, canvas_h), (255, 255, 255))
        offset_x = (canvas_w - new_w) // 2
        offset_y = (canvas_h - new_h) // 2
        canvas.paste(img_resized, (offset_x, offset_y))

        return canvas, None

    except Exception as e:
        return None, (file_path, str(e))
```

### Rust 対応
```rust
use image::{ImageBuffer, Rgb, RgbImage, DynamicImage};
use image::io::Reader as ImageReader;
use std::path::Path;

pub fn process_single_image(
    file_path: &Path,
    canvas_w: u32,
    canvas_h: u32,
) -> std::result::Result<RgbImage, ProcessingError> {
    // Load
    let mut img = ImageReader::open(file_path)?
        .decode()
        .map_err(|e| ProcessingError::new(file_path, &e.to_string()))?;

    // EXIF Rotation (image-exif クレート使用)
    // 実装例 - エラー時は無視
    // let img = apply_exif_rotation(img).ok();

    // Convert to RGB
    let img_rgb = img.to_rgb8();

    // Scale
    let (orig_w, orig_h) = (img_rgb.width(), img_rgb.height());
    let scale = f32::min(
        canvas_w as f32 / orig_w as f32,
        canvas_h as f32 / orig_h as f32,
    );
    let new_w = ((orig_w as f32 * scale) + 0.5) as u32;
    let new_h = ((orig_h as f32 * scale) + 0.5) as u32;

    let img_resized = image::imageops::resize(
        &img_rgb,
        new_w,
        new_h,
        image::imageops::FilterType::Lanczos3, // Bicubic相当
    );

    // Center on Canvas
    let mut canvas: RgbImage = ImageBuffer::new(canvas_w, canvas_h);
    
    // 背景色で埋める
    for pixel in canvas.pixels_mut() {
        *pixel = Rgb([255, 255, 255]);
    }

    let offset_x = ((canvas_w - new_w) / 2) as i64;
    let offset_y = ((canvas_h - new_h) / 2) as i64;

    image::imageops::overlay(&mut canvas, &img_resized, offset_x, offset_y);

    Ok(canvas)
}

#[derive(Clone, Debug)]
pub struct ProcessingError {
    pub file_path: String,
    pub message: String,
}

impl ProcessingError {
    pub fn new(path: &Path, msg: &str) -> Self {
        ProcessingError {
            file_path: path.to_string_lossy().to_string(),
            message: msg.to_string(),
        }
    }
}
```

### ポイント
- PIL.Image → `image` クレート（`ImageReader`, `DynamicImage`）
- `ImageOps.exif_transpose()` → image-exif クレート（エラー時は `.ok()` で無視）
- 例外処理 → `Result<T, E>` パターン
- Canvas 生成 → `ImageBuffer::new()`
- 配置 → `imageops::overlay()`

---

## 5. ImageProcessor - 並列処理

### Python
```python
def _run_thread(self, file_list, canvas_width, output_path):
    canvas_height = self.calculate_height(canvas_width)
    errors = []
    
    max_workers = os.cpu_count() / 2 or 4

    with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as executor:
        completed_count = 0
        total_count = len(file_list)
        
        processed_results = [None] * total_count
        future_to_index = {
            executor.submit(self.process_single_image, f, canvas_width, canvas_height): i 
            for i, f in enumerate(file_list)
        }

        for future in concurrent.futures.as_completed(future_to_index):
            idx = future_to_index[future]
            try:
                img, error = future.result()
                processed_results[idx] = (img, error)
            except Exception as exc:
                processed_results[idx] = (None, (file_list[idx], str(exc)))
            
            completed_count += 1
            self.callback_progress(completed_count, total_count, "processing")

    # Collect results
    success_images = []
    for i, res in enumerate(processed_results):
        img, error = res
        if img:
            success_images.append(img)
        else:
            errors.append(error)

    # ... PDF generation ...
```

### Rust 対応
```rust
use rayon::prelude::*;
use std::sync::{Arc, Mutex};

pub fn process_images_parallel(
    file_list: Vec<String>,
    canvas_width: u32,
    progress_tx: Option<&mpsc::Sender<ProgressUpdate>>,
) -> (Vec<RgbImage>, Vec<ProcessingError>) {
    let canvas_height = Self::calculate_height(canvas_width);
    let total = file_list.len();
    
    // 進捗カウント用 (スレッドセーフ)
    let counter = Arc::new(Mutex::new(0));

    // Rayon で並列処理（順序保持）
    let results: Vec<_> = file_list
        .par_iter()
        .enumerate()
        .map(|(idx, file_path)| {
            let result = Self::process_single_image(
                Path::new(file_path),
                canvas_width,
                canvas_height,
            );

            // 進捗送信
            if let Some(tx) = progress_tx {
                let mut count = counter.lock().unwrap();
                *count += 1;
                let _ = tx.send(ProgressUpdate {
                    count: *count,
                    total: total,
                    phase: ProgressPhase::Processing,
                });
            }

            (idx, result)
        })
        .collect();

    // 結果を分類
    let mut success_images = vec![None; total];
    let mut errors = Vec::new();

    for (idx, result) in results {
        match result {
            Ok(img) => success_images[idx] = Some(img),
            Err(e) => errors.push(e),
        }
    }

    let success_images = success_images
        .into_iter()
        .filter_map(|img| img)
        .collect();

    (success_images, errors)
}
```

### ポイント
- `concurrent.futures.ThreadPoolExecutor` → `rayon::prelude::par_iter()`
- `as_completed()` で進捗追跡 → `Arc<Mutex<>>` でカウント共有
- 例外キャッチ → `Result<T, E>` パターン
- スレッドセーフ： Rust の所有権により自動保証

---

## 6. ImageProcessor - PDF生成

### Python
```python
# Phase 2: PDF Generation
try:
    self.callback_progress(total_count, total_count, "saving")
    
    first_image = success_images[0]
    rest_images = success_images[1:]
    
    first_image.save(
        output_path,
        "PDF",
        resolution=72.0,
        save_all=True,
        append_images=rest_images,
        quality=80
    )
    
    self.callback_finished(True, len(success_images), len(errors), errors, output_path)

except Exception as e:
    final_errors = errors + [("PDF Generation", str(e))]
    self.callback_finished(False, len(success_images), len(final_errors), final_errors, output_path)
```

### Rust 対応
```rust
// Option 1: printpdf クレート使用
use printpdf::*;
use std::fs::File;
use std::io::BufWriter;

pub fn generate_pdf(
    images: Vec<RgbImage>,
    output_path: &str,
    progress_tx: Option<&mpsc::Sender<ProgressUpdate>>,
) -> Result<(), ProcessingError> {
    // 進捗通知
    if let Some(tx) = progress_tx {
        let _ = tx.send(ProgressUpdate {
            count: images.len(),
            total: images.len(),
            phase: ProgressPhase::Saving,
        });
    }

    let file = File::create(output_path)
        .map_err(|e| ProcessingError::new(Path::new("PDF"), &format!("Failed to create PDF: {}", e)))?;
    
    let writer = BufWriter::new(file);
    let (document, page1, layer1) = PdfDocument::new("ImageToPdf", Mm(A4_WIDTH), Mm(A4_HEIGHT), "Layer 1");

    // 各画像をページとして追加
    for (page_idx, image) in images.iter().enumerate() {
        if page_idx > 0 {
            let (page, layer) = document.add_page(Mm(A4_WIDTH), Mm(A4_HEIGHT), "Page");
            // 画像を配置
            // ... 実装 ...
        } else {
            // 最初のページは page1, layer1 を使用
        }
    }

    document.save(writer)
        .map_err(|e| ProcessingError::new(Path::new("PDF"), &format!("Failed to save PDF: {}", e)))?;

    Ok(())
}

// Option 2: pdfium-render クレート使用 (より高度)
```

### ポイント
- PIL.Image.save() → `printpdf` or `pdfium-render` クレート
- 例外処理 → `Result<T, E>` で返値化
- PDFライブラリの選択が重要（互換性、機能）

---

## 7. MainApp クラス - GUI初期化

### Python (tkinter)
```python
class MainApp(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("画像結合PDF生成ツール")
        self.geometry("600x500")
        
        self.config = AppConfig()
        self.processor = ImageProcessor(self.update_progress, self.on_finished)
        
        self.file_list = []
        self.canvas_width_var = tk.IntVar(value=DEFAULT_WIDTH)
        self.canvas_width_var.trace_add("write", self.on_width_change)
        
        self.setup_ui()
        self.update_ui_state()
```

### Rust 対応 (FLTK-rs の例)
```rust
// src/gui/fltk_app.rs

use fltk::{prelude::*, *};
use std::sync::{Arc, Mutex};
use crate::app_config::AppConfig;
use crate::image_processor::ImageProcessor;

pub struct AppWindow {
    wind: window::Window,
    file_list: Arc<Mutex<Vec<String>>>,
    canvas_width: Arc<Mutex<u32>>,
    config: Arc<Mutex<AppConfig>>,
}

impl AppWindow {
    pub fn new() -> Self {
        let mut wind = window::Window::default()
            .with_size(600, 500)
            .with_label("画像結合PDF生成ツール");

        // UI 構築
        Self::setup_ui(&mut wind);

        wind.end();
        wind.show();

        AppWindow {
            wind,
            file_list: Arc::new(Mutex::new(Vec::new())),
            canvas_width: Arc::new(Mutex::new(DEFAULT_WIDTH)),
            config: Arc::new(Mutex::new(AppConfig::new().unwrap())),
        }
    }

    fn setup_ui(wind: &mut window::Window) {
        // 1. Input Area
        let mut input_frame = frame::Frame::default()
            .with_size(580, 100)
            .with_pos(10, 10)
            .with_label("1. 入力");
        
        let mut btn1 = button::Button::default()
            .with_size(100, 25)
            .with_pos(20, 30);
        btn1.set_label("フォルダを選択");

        let mut btn2 = button::Button::default()
            .with_size(100, 25)
            .with_pos(130, 30);
        btn2.set_label("ファイルを追加");

        // ... 続く ...
    }

    pub fn run(&mut self) {
        app::App::default().run().unwrap();
    }
}
```

### ポイント
- `tk.Tk` → FLTK-rs の `window::Window`
- state 管理 → `Arc<Mutex<T>>` でスレッド間共有
- コールバック → クロージャまたはメッセージチャネル

---

## 8. MainApp - ファイル選択

### Python
```python
def select_folder(self):
    folder = filedialog.askdirectory()
    if folder:
        files = []
        folder_path = Path(folder)
        for f in folder_path.iterdir():
            if f.is_file() and f.suffix.lower() in ['.jpg', '.jpeg']:
                files.append(str(f))
        
        files.sort(key=lambda s: Path(s).name.lower())
        
        self.file_list = files
        self.update_input_status(f"{len(files)}枚の画像が見つかりました")
        self.update_ui_state()

def add_files(self):
    files = filedialog.askopenfilenames(
        filetypes=[("JPEG files", "*.jpg *.jpeg"), ("All files", "*.*")]
    )
    if files:
        sorted_files = sorted(files, key=lambda s: Path(s).name.lower())
        self.file_list = sorted_files
        self.update_input_status(f"{len(sorted_files)}枚の画像を選択")
        self.update_ui_state()
```

### Rust 対応
```rust
use std::path::Path;

pub fn select_folder(file_list: &Arc<Mutex<Vec<String>>>) {
    // FLTK or nfd (native file dialog) クレート使用
    if let Ok(path) = nfd::open_pick_folder(None) {
        if let Ok(path) = path {
            let folder_path = Path::new(&path);
            let mut files = Vec::new();

            if let Ok(entries) = std::fs::read_dir(folder_path) {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let path = entry.path();
                        if path.is_file() {
                            if let Some(ext) = path.extension() {
                                let ext = ext.to_string_lossy().to_lowercase();
                                if ext == "jpg" || ext == "jpeg" {
                                    files.push(path.to_string_lossy().to_string());
                                }
                            }
                        }
                    }
                }
            }

            // ソート
            files.sort_by(|a, b| {
                let a_name = Path::new(a).file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                let b_name = Path::new(b).file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                a_name.cmp(&b_name)
            });

            *file_list.lock().unwrap() = files;
        }
    }
}

pub fn add_files(file_list: &Arc<Mutex<Vec<String>>>) {
    // nfd クレート使用
    if let Ok(files) = nfd::open_file_multiple_pick(Some("jpg,jpeg"), None) {
        if let Ok(files) = files {
            let mut file_vec: Vec<String> = files
                .iter()
                .map(|f| f.to_string_lossy().to_string())
                .collect();

            file_vec.sort_by(|a, b| {
                let a_name = Path::new(a).file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                let b_name = Path::new(b).file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                a_name.cmp(&b_name)
            });

            *file_list.lock().unwrap() = file_vec;
        }
    }
}
```

### ポイント
- `filedialog.askdirectory()` → `nfd` クレート or `rfd` クレート
- ファイル列挙 → `std::fs::read_dir()`
- ソート → `sort_by()` メソッド

---

## 9. MainApp - 進捗とコールバック

### Python
```python
def update_progress(self, count, total, phase):
    # スレッドセーフ: self.after() で GUI スレッドに移譲
    self.after(0, self._update_progress_ui, count, total, phase)

def _update_progress_ui(self, count, total, phase):
    if phase == "processing":
        self.progress_bar['value'] = (count / total) * 90
        self.status_label.config(text=f"画像処理中 ({count}/{total})")
    elif phase == "saving":
        self.progress_bar['value'] = 95
        self.status_label.config(text="PDF書き出し中...")

def on_finished(self, success, success_count, error_count, errors, output_path):
    self.after(0, self._on_finished_ui, success, success_count, error_count, errors, output_path)
```

### Rust 対応
```rust
use std::sync::mpsc;

#[derive(Clone, Debug)]
pub enum Message {
    UpdateProgress { count: usize, total: usize, phase: String },
    ProcessingFinished { success: bool, success_count: usize, error_count: usize, errors: Vec<String>, output_path: String },
}

pub fn handle_progress_message(msg: Message) {
    match msg {
        Message::UpdateProgress { count, total, phase } => {
            if phase == "processing" {
                let progress = (count as f32 / total as f32) * 90.0;
                // GUI 更新
                println!("画像処理中 ({}/{})", count, total);
            } else if phase == "saving" {
                // GUI 更新
                println!("PDF書き出し中...");
            }
        }
        Message::ProcessingFinished { success, success_count, error_count, errors, output_path } => {
            // 完了処理
            if success {
                println!("PDFを保存しました: {}", output_path);
            } else {
                println!("処理失敗");
            }
        }
    }
}

// スレッド内で送信
let (tx, rx) = mpsc::channel();

std::thread::spawn(move || {
    // ... 処理 ...
    let _ = tx.send(Message::UpdateProgress { 
        count: 50, 
        total: 100, 
        phase: "processing".to_string() 
    });
});

// メインスレッドで受信
for msg in rx.iter() {
    handle_progress_message(msg);
}
```

### ポイント
- `self.after()` スレッドセーフ処理 → `mpsc::channel()` で通信
- GUI 更新 → メッセージパッシング
- 型安全性： Rust は状態をコンパイル時に検証

---

## 10. エラーハンドリング比較

### Python
```python
try:
    img = Image.open(file_path)
except Exception as e:
    return None, (file_path, str(e))

try:
    img = ImageOps.exif_transpose(img)
except Exception:
    pass  # 無視

try:
    with open(self.config_path, "r", encoding="utf-8") as f:
        data = json.load(f)
except Exception:
    pass
```

### Rust 対応
```rust
// 致命的エラーは Result で返す
pub fn process_image(path: &Path) -> Result<RgbImage, ProcessingError> {
    let img = ImageReader::open(path)?
        .decode()
        .map_err(|e| ProcessingError::new(path, &e.to_string()))?;
    Ok(img.to_rgb8())
}

// 回復可能なエラーは .ok() or .unwrap_or() で無視
let img_with_exif = apply_exif_rotation(img).ok();

// 設定読み込み時はデフォルト値を返す
let config = load_config().unwrap_or_default();

// カスタムエラー型
#[derive(Debug)]
pub enum AppError {
    IoError(std::io::Error),
    ImageError(String),
    PdfError(String),
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::IoError(err)
    }
}
```

### ポイント
- Python の except は Rust では Result
- 明示的なエラー処理が必要
- `?` 演算子で early return
- カスタムエラー型で詳細な情報管理

---

## 11. メモリ管理の比較

### Python（ガベージコレクション）
```python
# メモリ自動管理
img = Image.open(file_path)  # メモリ確保
# 関数スコープ出ると自動解放
```

### Rust（所有権）
```rust
// 明示的な所有権管理
let img = ImageReader::open(file_path)?.decode()?;
let img_rgb = img.to_rgb8();  // img は img_rgb によって move

// メモリ使用量の見積もり
// RgbImage: width * height * 3 bytes
// 2000x1500 pixels → 9 MB per image
// 100 images → 900 MB peak

// 改善例： ストリーミング処理
for file in file_list {
    let img = process_one_image(&file)?;
    save_to_pdf_stream(&mut pdf, img)?;
    // img はここで drop (メモリ解放)
}
```

### ポイント
- Python：メモリ管理が自動だが予測困難
- Rust：明示的、から も効率的（メモリリークなし）

---

## 12. 並列処理の比較

### Python（スレッド）
```python
max_workers = os.cpu_count() / 2 or 4
with concurrent.futures.ThreadPoolExecutor(max_workers=max_workers) as executor:
    futures = [executor.submit(process_task, item) for item in items]
    for future in concurrent.futures.as_completed(futures):
        result = future.result()
```

### Rust（Rayon）
```rust
use rayon::prelude::*;

let max_workers = num_cpus::get() / 2 min 4;
rayon::ThreadPoolBuilder::new()
    .num_threads(max_workers)
    .build()
    .unwrap()
    .install(|| {
        let results: Vec<_> = items
            .par_iter()
            .map(|item| process_task(item))
            .collect();
    });
```

### ポイント
- Python：GIL の影響（I/O待機時は有効）
- Rust：GILなし、CPU バウンド処理に最適
- Rayon：簡潔な API

---

## 13. テスト記述の比較

### Python
```python
def test_calculate_height():
    processor = ImageProcessor(None, None)
    height = processor.calculate_height(1654)
    assert height == 2336

def test_process_single_image():
    processor = ImageProcessor(None, None)
    img, error = processor.process_single_image("path/to/test.jpg", 1654, 2336)
    assert img is not None
    assert error is None
```

### Rust
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_height() {
        let height = ImageProcessor::calculate_height(1654);
        assert_eq!(height, 2336);
    }

    #[test]
    fn test_process_single_image() {
        let path = Path::new("tests/fixtures/test.jpg");
        let result = ImageProcessor::process_single_image(path, 1654, 2336);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_parallel_processing() {
        // 非同期テスト
    }
}
```

### ポイント
- Rust：`#[test]` マクロで直列テスト
- `#[tokio::test]` で非同期テスト
- 仕様から Rust は型チェックで多くのバグを事前に検出

---

## 14. 依存関係の紹介

| Python ライブラリ | Rust クレート | 機能 |
|------------------|------------|------|
| PIL/Pillow | `image` | 画像読込/処理 |
| PIL/Pillow | `printpdf` or `pdfium-render` | PDF生成 |
| tkinter | `fltk-rs` / `iced` / `gtk-rs` | GUI |
| json | `serde_json` | JSON |
| threading | `std::thread` + `Arc/Mutex` | スレッド |
| concurrent.futures | `rayon` | 並列処理 |
| pathlib | `std::path` | パス操作 |
| os | `std::env` | 環境変数 |
| × | `dirs` | クロスプラットフォーム |
| × | `thiserror` / `anyhow` | エラー処理 |

---

## 15. パフォーマンス比較（推定）

| 処理 | Python | Rust | 比率 |
|------|--------|------|------|
| JPEG読み込み | 15ms | 12ms | 1.25x |
| リサイズ | 40ms | 30ms | 1.33x |
| PDF生成 100ページ | 800ms | 400ms | 2.0x |
| **全処理（100枚）** | 2500ms | 1200ms | 2.08x |

**注:** 推定値、実測データは environment に依存

---

## 16. 移行のチェックリスト

- [ ] `AppConfig` を Rust で実装
- [ ] `ImageProcessor.calculate_height()` テスト
- [ ] `process_single_image()` 単一画像処理テスト
- [ ] 並列処理統合（rayon）
- [ ] PDF生成機能テスト
- [ ] GUI フレームワーク選定
- [ ] ファイルダイアログ統合
- [ ] 進捗表示統合
- [ ] エラーハンドリング完全検証
- [ ] クロスプラットフォーム テスト（Windows/macOS/Linux）
- [ ] パフォーマンス最適化
- [ ] ドキュメント作成

---

## 参考資料

- **Rust Book:** https://doc.rust-lang.org/book/
- **image クレート:** https://docs.rs/image/
- **rayon:** https://docs.rs/rayon/
- **FLTK-rs:** https://fltk-rs.github.io/fltk-book/
