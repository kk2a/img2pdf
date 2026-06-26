//! FLTK ベースの GUI 実装
//!
//! ## UI レイアウト
//! ```text
//! ┌─ 1. 入力 ─────────────────────────────┐
//! │ [フォルダを選択] [ファイルを追加]       │
//! │ 対象ファイル数: 0 枚                   │
//! ├─ 2. 出力 ─────────────────────────────┤
//! │ [保存先を選択] [未選択................] │
//! ├─ 3. 設定 ─────────────────────────────┤
//! │ キャンバス幅 (px): [1654]             │
//! │ キャンバス高さ: 2339 px               │
//! │ [✓] 最大性能モードを有効化            │
//! │              [PDF 生成を実行]          │
//! ├─ 4. 進捗 ─────────────────────────────┤
//! │ [████████░░░░░░░░░░░] 50%            │
//! │ 画像処理中 (5/10)                     │
//! └───────────────────────────────────────┘
//! ```

use crate::app_config::AppConfig;
use crate::image_processor::{init_thread_pool, ImageProcessor};
use crate::models::{ProcessingResult, ProgressPhase, ProgressUpdate};
use crate::utils::constants::*;
use fltk::{
    app,
    button::{Button, CheckButton},
    dialog,
    enums::{Align, Color, FrameType},
    frame::Frame,
    input::IntInput,
    misc::Progress,
    prelude::*,
    window::Window,
};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// GUI スレッド間通信メッセージ
#[derive(Clone, Debug)]
enum AppMessage {
    /// ファイルリストが更新された（ファイル数）
    FilesUpdated(usize),
    /// 出力パスが確定した
    OutputSelected(String),
    /// キャンバス幅が変更された
    WidthChanged(u32),
    /// 進捗更新
    Progress(ProgressUpdate),
    /// 処理完了
    Finished(ProcessingResult),
}

/// アプリケーション状態
struct AppState {
    file_list: Vec<String>,
    output_path: String,
    canvas_width: u32,
    max_performance: bool,
    config: AppConfig,
}

impl AppState {
    fn new() -> Self {
        let config = AppConfig::new().unwrap_or_else(|_| {
            // 設定ファイルが読み込めない場合はメモリ上のデフォルト値で起動
            // AppConfig には必ず valid な状態を持たせる
            AppConfig::default()
        });
        let canvas_width = DEFAULT_WIDTH;
        AppState {
            file_list: Vec::new(),
            output_path: String::new(),
            canvas_width,
            max_performance: false,
            config,
        }
    }
}

/// FLTK アプリケーションを起動する
pub fn run() {
    let app = app::App::default().with_scheme(app::Scheme::Gtk);
    let (sender, receiver) = app::channel::<AppMessage>();

    // 共有状態
    let state = Arc::new(Mutex::new(AppState::new()));

    // ── ウィンドウ構築 ──────────────────────────────────────────────────
    let mut wind = Window::default()
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_label("画像結合PDF生成ツール");
    wind.set_color(Color::from_rgb(245, 245, 245));

    // ── 1. 入力セクション ────────────────────────────────────────────────
    let mut section1 = Frame::default()
        .with_size(WINDOW_WIDTH - 20, 80)
        .with_pos(10, 5)
        .with_label("1. 入力");
    section1.set_align(Align::TopLeft | Align::Inside);
    section1.set_frame(FrameType::EngravedBox);

    let mut btn_folder = Button::default()
        .with_size(140, 28)
        .with_pos(20, 25)
        .with_label("フォルダを選択");

    let mut btn_files = Button::default()
        .with_size(140, 28)
        .with_pos(170, 25)
        .with_label("ファイルを追加");

    let mut lbl_file_count = Frame::default()
        .with_size(220, 25)
        .with_pos(20, 58)
        .with_label("対象ファイル数: 0 枚");
    lbl_file_count.set_align(Align::Left | Align::Inside);

    // ── 2. 出力セクション ────────────────────────────────────────────────
    let mut section2 = Frame::default()
        .with_size(WINDOW_WIDTH - 20, 70)
        .with_pos(10, 95)
        .with_label("2. 出力");
    section2.set_align(Align::TopLeft | Align::Inside);
    section2.set_frame(FrameType::EngravedBox);

    let mut btn_output = Button::default()
        .with_size(140, 28)
        .with_pos(20, 120)
        .with_label("保存先を選択");

    let mut lbl_output = Frame::default()
        .with_size(390, 28)
        .with_pos(170, 120)
        .with_label("未選択");
    lbl_output.set_align(Align::Left | Align::Inside);
    lbl_output.set_frame(FrameType::FlatBox);
    lbl_output.set_color(Color::from_rgb(230, 230, 230));

    // ── 3. 設定セクション ────────────────────────────────────────────────
    let mut section3 = Frame::default()
        .with_size(WINDOW_WIDTH - 20, 135)
        .with_pos(10, 175)
        .with_label("3. 設定");
    section3.set_align(Align::TopLeft | Align::Inside);
    section3.set_frame(FrameType::EngravedBox);

    let mut lbl_width = Frame::default()
        .with_size(170, 25)
        .with_pos(20, 200)
        .with_label("キャンバス幅 (px):");
    lbl_width.set_align(Align::Left | Align::Inside);

    let initial_width = {
        let s = state.lock().unwrap();
        s.canvas_width
    };

    let mut input_width = IntInput::default().with_size(100, 25).with_pos(195, 200);
    input_width.set_value(&initial_width.to_string());

    let mut lbl_height = Frame::default()
        .with_size(WINDOW_WIDTH - 40, 25)
        .with_pos(20, 228)
        .with_label(&format!(
            "キャンバス高さ: {} px",
            ImageProcessor::calculate_height(initial_width)
        ));
    lbl_height.set_align(Align::Left | Align::Inside);

    let mut chk_max_performance = CheckButton::default()
        .with_size(WINDOW_WIDTH - 40, 25)
        .with_pos(20, 254)
        .with_label("最大性能モードを有効化（全 CPU コアを使用）");
    chk_max_performance.set_value(false);

    let mut btn_run = Button::default()
        .with_size(200, 32)
        .with_pos((WINDOW_WIDTH - 200) / 2, 273)
        .with_label("PDF 生成を実行");
    btn_run.set_color(Color::from_rgb(70, 130, 180));
    btn_run.set_label_color(Color::White);
    btn_run.deactivate();

    // ── 4. 進捗セクション ────────────────────────────────────────────────
    let mut section4 = Frame::default()
        .with_size(WINDOW_WIDTH - 20, 90)
        .with_pos(10, 320)
        .with_label("4. 進捗");
    section4.set_align(Align::TopLeft | Align::Inside);
    section4.set_frame(FrameType::EngravedBox);

    let mut progress_bar = Progress::default()
        .with_size(WINDOW_WIDTH - 40, 28)
        .with_pos(20, 342);
    progress_bar.set_minimum(0.0);
    progress_bar.set_maximum(100.0);
    progress_bar.set_value(0.0);
    progress_bar.set_color(Color::from_rgb(200, 200, 200));
    progress_bar.set_selection_color(Color::from_rgb(70, 130, 180));

    let mut lbl_status = Frame::default()
        .with_size(WINDOW_WIDTH - 40, 25)
        .with_pos(20, 374)
        .with_label("待機中");
    lbl_status.set_align(Align::Left | Align::Inside);

    wind.end();
    wind.show();

    // ── コールバック: フォルダを選択 ─────────────────────────────────────
    {
        let state_c = Arc::clone(&state);
        let sender_c = sender.clone();
        btn_folder.set_callback(move |_| {
            let initial_dir = {
                let s = state_c.lock().unwrap();
                s.config.get_last_input_dir().to_string()
            };
            let mut chooser =
                dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseDir);
            chooser.set_title("処理するフォルダを選択してください");
            if !initial_dir.is_empty() {
                let _ = chooser.set_directory(&initial_dir);
            }
            chooser.show();

            let folder = chooser.filename().to_string_lossy().to_string();
            if !folder.is_empty() {
                let mut files = collect_jpeg_files(Path::new(&folder));
                files.sort_by(|a, b| {
                    let a_name = Path::new(a)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    let b_name = Path::new(b)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    a_name.cmp(&b_name)
                });
                let count = files.len();
                {
                    let mut s = state_c.lock().unwrap();
                    s.config.set_last_input_dir(folder.clone());
                    s.config.save();
                    s.file_list = files;
                }
                sender_c.send(AppMessage::FilesUpdated(count));
            }
        });
    }

    // ── コールバック: ファイルを追加 ─────────────────────────────────────
    {
        let state_c = Arc::clone(&state);
        let sender_c = sender.clone();
        btn_files.set_callback(move |_| {
            let mut chooser =
                dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseMultiFile);
            chooser.set_title("JPEG ファイルを選択");
            chooser.set_filter("JPEG Files\t*.{jpg,jpeg}");
            chooser.show();

            let mut files: Vec<String> = chooser
                .filenames()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|s| {
                    if s.is_empty() {
                        return false;
                    }
                    let ext = Path::new(s)
                        .extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    ext == "jpg" || ext == "jpeg"
                })
                .collect();

            if !files.is_empty() {
                files.sort_by(|a, b| {
                    let a_name = Path::new(a)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    let b_name = Path::new(b)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase();
                    a_name.cmp(&b_name)
                });
                let count = files.len();
                state_c.lock().unwrap().file_list = files;
                sender_c.send(AppMessage::FilesUpdated(count));
            }
        });
    }

    // ── コールバック: 保存先を選択 ───────────────────────────────────────
    {
        let state_c = Arc::clone(&state);
        let sender_c = sender.clone();
        btn_output.set_callback(move |_| {
            let initial_dir = {
                let s = state_c.lock().unwrap();
                s.config.get_last_save_dir().to_string()
            };
            let mut chooser =
                dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseSaveFile);
            chooser.set_title("PDF 保存先を選択");
            chooser.set_filter("PDF Files\t*.pdf");
            if !initial_dir.is_empty() {
                let _ = chooser.set_directory(&initial_dir);
            }
            chooser.show();

            let path_str = chooser.filename().to_string_lossy().to_string();
            if !path_str.is_empty() {
                // .pdf 拡張子がなければ付与
                let path_with_ext = if path_str.to_lowercase().ends_with(".pdf") {
                    path_str
                } else {
                    format!("{path_str}.pdf")
                };
                sender_c.send(AppMessage::OutputSelected(path_with_ext));
            }
        });
    }

    // ── コールバック: 最大性能モード ─────────────────────────────────────
    {
        let state_c = Arc::clone(&state);
        chk_max_performance.set_callback(move |chk| {
            state_c.lock().unwrap().max_performance = chk.value();
        });
    }

    // ── コールバック: PDF 生成 ───────────────────────────────────────────
    {
        let state_c = Arc::clone(&state);
        let sender_c = sender.clone();
        btn_run.set_callback(move |btn| {
            let (file_list, canvas_width, output_path, max_performance) = {
                let s = state_c.lock().unwrap();
                (
                    s.file_list.clone(),
                    s.canvas_width,
                    s.output_path.clone(),
                    s.max_performance,
                )
            };

            if file_list.is_empty() {
                dialog::alert_default("処理するファイルが選択されていません。");
                return;
            }
            if output_path.is_empty() {
                dialog::alert_default("保存先が選択されていません。");
                return;
            }

            // 最大性能モードを反映してスレッドプールを初期化する
            // （初回のみ有効。以降の呼び出しは rayon により無視される）
            ImageProcessor::set_max_performance_mode(max_performance);
            init_thread_pool();

            btn.deactivate();

            // 進捗チャネル
            let (progress_tx, progress_rx) = mpsc::channel::<ProgressUpdate>();
            let (finished_tx, finished_rx) = mpsc::channel::<ProcessingResult>();

            // 進捗リレースレッド（mpsc → FLTK チャネル）
            let sender_progress = sender_c.clone();
            std::thread::spawn(move || {
                for update in progress_rx {
                    sender_progress.send(AppMessage::Progress(update));
                }
            });

            let sender_finished = sender_c.clone();
            std::thread::spawn(move || {
                if let Ok(result) = finished_rx.recv() {
                    sender_finished.send(AppMessage::Finished(result));
                }
            });

            ImageProcessor::run(
                file_list,
                canvas_width,
                output_path,
                Some(progress_tx),
                finished_tx,
            );
        });
    }

    // ── イベントループ ───────────────────────────────────────────────────
    while app.wait() {
        if let Some(msg) = receiver.recv() {
            match msg {
                AppMessage::FilesUpdated(count) => {
                    lbl_file_count.set_label(&format!("対象ファイル数: {count} 枚"));
                    update_run_button(&state, &mut btn_run);
                }
                AppMessage::OutputSelected(path) => {
                    let display = truncate_path(&path, 45);
                    lbl_output.set_label(&display);
                    {
                        let mut s = state.lock().unwrap();
                        if let Some(dir) = Path::new(&path).parent() {
                            s.config
                                .set_last_save_dir(dir.to_string_lossy().to_string());
                            s.config.save();
                        }
                        s.output_path = path;
                    }
                    update_run_button(&state, &mut btn_run);
                }
                AppMessage::WidthChanged(w) => {
                    lbl_height.set_label(&format!(
                        "キャンバス高さ: {} px",
                        ImageProcessor::calculate_height(w)
                    ));
                    state.lock().unwrap().canvas_width = w;
                }
                AppMessage::Progress(update) => {
                    match update.phase {
                        ProgressPhase::Processing => {
                            let pct = (update.count as f64 / update.total as f64) * 90.0;
                            progress_bar.set_value(pct);
                            lbl_status.set_label(&format!(
                                "画像処理中 ({}/{})",
                                update.count, update.total
                            ));
                        }
                        ProgressPhase::Saving => {
                            progress_bar.set_value(95.0);
                            lbl_status.set_label("PDF 書き出し中...");
                        }
                    }
                    app::flush();
                }
                AppMessage::Finished(result) => {
                    progress_bar.set_value(100.0);
                    btn_run.activate();

                    if result.success {
                        let msg = format!(
                            "PDF を保存しました。\n成功: {} 枚 / エラー: {} 枚\n保存先: {}",
                            result.success_count, result.error_count, result.output_path
                        );
                        lbl_status.set_label("完了");
                        dialog::message_default(&msg);
                    } else {
                        let error_details: Vec<String> = result
                            .errors
                            .iter()
                            .map(|e| format!("  {}: {}", e.file_path, e.message))
                            .collect();
                        let msg = format!("処理に失敗しました。\n\n{}", error_details.join("\n"));
                        lbl_status.set_label("エラー");
                        dialog::alert_default(&msg);
                    }
                }
            }
        }

        // キャンバス幅の入力変更を検知
        let w_str = input_width.value();
        if let Ok(w) = w_str.parse::<u32>() {
            if w > 0 {
                let current = state.lock().unwrap().canvas_width;
                if current != w {
                    sender.send(AppMessage::WidthChanged(w));
                }
            }
        }
    }
}

/// フォルダ内の JPEG ファイルを収集する（再帰なし）
fn collect_jpeg_files(folder: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
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
    files
}

/// 長いパスを末尾から指定文字数に切り詰めて表示する
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - max_len..])
    }
}

/// ファイルリストと出力パスが揃っていれば実行ボタンを有効化する
fn update_run_button(state: &Arc<Mutex<AppState>>, btn: &mut Button) {
    let s = state.lock().unwrap();
    if !s.file_list.is_empty() && !s.output_path.is_empty() {
        btn.activate();
    } else {
        btn.deactivate();
    }
}
