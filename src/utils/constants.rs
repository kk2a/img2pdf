/// アプリケーション名
pub const APP_NAME: &str = "ImageToPdf";

/// 設定ディレクトリ名
pub const CONFIG_DIR_NAME: &str = "ImageToPdf";

/// 設定ファイル名
pub const CONFIG_FILE_NAME: &str = "config.json";

/// デフォルトキャンバス幅（ピクセル） — A4 @ 約200 DPI
pub const DEFAULT_WIDTH: u32 = 1654;

/// A4用紙 高さ/幅 比率 (√2)
pub const A4_RATIO: f32 = 1.41421356;

/// PDF DPI
pub const PDF_RESOLUTION: f64 = 72.0;

/// PDF品質（1–95）
pub const PDF_QUALITY: u8 = 80;

/// キャンバス背景色（RGB）
pub const CANVAS_COLOR: [u8; 3] = [255, 255, 255];

/// 対応ファイル拡張子
pub const SUPPORTED_EXTENSIONS: &[&str] = &["jpg", "jpeg"];

/// GUIウィンドウ幅
pub const WINDOW_WIDTH: i32 = 600;

/// GUIウィンドウ高さ
pub const WINDOW_HEIGHT: i32 = 420;
