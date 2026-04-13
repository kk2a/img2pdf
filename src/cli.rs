//! CLI（ヘッドレス）モード
//!
//! ディスプレイなしでも動作する。コマンドライン引数を解析して
//! ImageProcessor を直接呼び出す。
//!
//! ## 使用方法
//! ```text
//! img2pdf <input_folder> [output.pdf] [options]
//!
//! Options:
//!   --width <px>    キャンバス幅 (デフォルト: 1654)
//!   --max-performance   最大性能モードを有効化（既定）
//!   --no-max-performance 最大性能モードを無効化
//!   --help          このヘルプを表示
//! ```

use crate::image_processor::ImageProcessor;
use crate::models::{ProcessingResult, ProgressPhase, ProgressUpdate};
use crate::utils::constants::DEFAULT_WIDTH;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

/// CLI 引数
#[derive(Debug)]
pub struct CliArgs {
    pub input_folder: String,
    pub output_path: String,
    pub canvas_width: u32,
    pub max_performance: bool,
}

/// コマンドライン引数を解析する
///
/// 引数が不足している場合は `None` を返す。
pub fn parse_args(args: &[String]) -> Option<CliArgs> {
    if args.is_empty() {
        return None;
    }

    // --help
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        std::process::exit(0);
    }

    // 先頭引数は必須: <input_folder>
    let input_folder = args[0].clone();
    let output_path = match args.get(1) {
        Some(arg) if !arg.starts_with("--") => arg.clone(),
        _ => default_output_path(),
    };

    // --width オプション（省略時はデフォルト）
    let canvas_width = args
        .windows(2)
        .find(|w| w[0] == "--width")
        .and_then(|w| w[1].parse::<u32>().ok())
        .unwrap_or(DEFAULT_WIDTH);

    let max_performance = if args.iter().any(|a| a == "--no-max-performance") {
        false
    } else {
        true
    };

    Some(CliArgs {
        input_folder,
        output_path,
        canvas_width,
        max_performance,
    })
}

/// ヘルプを表示する
fn print_usage() {
    eprintln!(
                "使用方法: img2pdf <input_folder> [output.pdf] [--width <px>] [--max-performance|--no-max-performance]

引数:
  <input_folder>    処理する JPEG 画像が入ったフォルダ
    [output.pdf]      出力 PDF ファイルパス（省略時: output-[seed].pdf）

オプション:
  --width <px>      キャンバス幅（ピクセル）[デフォルト: {}]
    --max-performance  最大性能モード（全CPU使用）[デフォルト]
    --no-max-performance 最大性能モードを無効化
  --help, -h        このヘルプを表示

引数なしで起動すると GUI モードで起動します。",
        DEFAULT_WIDTH
    );
}

/// 既定の出力先 `output-[seed].pdf` を現在ディレクトリに生成する
fn default_output_path() -> String {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = format!("output-{seed}.pdf");
    PathBuf::from(file_name).to_string_lossy().to_string()
}

/// CLI モードでの処理を実行する（ブロッキング）
///
/// 成功時は `Ok(ProcessingResult)` を返す。
pub fn run(args: &CliArgs) -> ProcessingResult {
    ImageProcessor::set_max_performance_mode(args.max_performance);
    if args.max_performance {
        eprintln!("最大性能モード: ON");
    }

    // フォルダから JPEG 収集
    let mut file_list = collect_jpeg_files(&args.input_folder);
    file_list.sort_by(|a, b| {
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

    if file_list.is_empty() {
        eprintln!("エラー: フォルダ内に JPEG ファイルが見つかりません: {}", args.input_folder);
        return ProcessingResult {
            success: false,
            success_count: 0,
            error_count: 1,
            errors: vec![crate::models::ProcessingError {
                file_path: args.input_folder.clone(),
                message: "No JPEG files found".to_string(),
            }],
            output_path: args.output_path.clone(),
        };
    }

    eprintln!("{} 枚の JPEG を検出しました。処理を開始します...", file_list.len());

    // 進捗チャネル
    let (progress_tx, progress_rx) = mpsc::channel::<ProgressUpdate>();
    let (finished_tx, finished_rx) = mpsc::channel::<ProcessingResult>();

    // 進捗表示スレッド
    let total_for_display = file_list.len();
    let progress_handle = std::thread::spawn(move || {
        let mut has_output = false;
        for update in progress_rx {
            let (label, count, total) = match update.phase {
                ProgressPhase::Processing => ("画像処理", update.count, update.total),
                ProgressPhase::Saving => ("PDF保存", total_for_display, total_for_display),
            };

            let line = render_progress_line(label, count, total);
            eprint!("\r{line}");
            let _ = io::stderr().flush();
            has_output = true;
        }

        if has_output {
            eprintln!();
        }
    });

    ImageProcessor::run(
        file_list,
        args.canvas_width,
        args.output_path.clone(),
        Some(progress_tx),
        finished_tx,
    );

    // 完了まで待機（ブロッキング）
    let result = finished_rx.recv().unwrap_or_else(|_| ProcessingResult {
        success: false,
        success_count: 0,
        error_count: 1,
        errors: vec![crate::models::ProcessingError {
            file_path: "internal".to_string(),
            message: "Processing thread disconnected".to_string(),
        }],
        output_path: args.output_path.clone(),
    });

    let _ = progress_handle.join();
    result
}

/// CLI 用の 1 行プログレス表示文字列を作る
fn render_progress_line(label: &str, count: usize, total: usize) -> String {
    let safe_total = total.max(1);
    let pct = (count.min(safe_total) * 100) / safe_total;
    let width = 30usize;
    let filled = (pct * width) / 100;
    let bar = format!("{}{}", "#".repeat(filled), "-".repeat(width - filled));
    format!("  {label}: [{bar}] {pct:>3}% ({count}/{total})")
}

/// フォルダ内の JPEG ファイルを収集する（再帰なし）
fn collect_jpeg_files(folder: &str) -> Vec<String> {
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
