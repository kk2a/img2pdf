//! CLI（ヘッドレス）モード
//!
//! ディスプレイなしでも動作する。コマンドライン引数を解析して
//! ImageProcessor / Pdf2ImgProcessor を直接呼び出す。
//!
//! ## 使用方法
//! ```text
//! img2pdf <input_folder> [output.pdf] [options]
//! img2pdf pdf2img <input.pdf> [output_folder] [options]
//! pdf2img <input.pdf> [output_folder] [options]
//! ```

use crate::image_processor::ImageProcessor;
use crate::models::{ProcessingError, ProcessingResult, ProgressPhase, ProgressUpdate};
use crate::pdf2img_processor::{OutputImageFormat, Pdf2ImgProcessor};
use crate::utils::constants::DEFAULT_WIDTH;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

/// CLI 動作モード
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliMode {
    Img2Pdf,
    Pdf2Img,
}

/// コマンドライン引数
#[derive(Debug)]
pub struct CliArgs {
    pub mode: CliMode,
    pub input_folder: String,
    pub output_path: String,
    pub canvas_width: u32,
    pub max_performance: bool,
    pub output_format: OutputImageFormat,
    pub lossless: bool,
}

/// コマンドライン引数を解析する
///
/// 引数が不足している場合は `None` を返す。
pub fn parse_args(args: &[String]) -> Option<CliArgs> {
    if args.is_empty() {
        return None;
    }

    if args[0] == "pdf2img" {
        return parse_pdf2img_args(&args[1..]);
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        std::process::exit(0);
    }

    parse_img2pdf_args(args)
}

fn parse_img2pdf_args(args: &[String]) -> Option<CliArgs> {
    if args.is_empty() {
        return None;
    }

    let input_folder = args[0].clone();
    let output_path = match args.get(1) {
        Some(arg) if !arg.starts_with("--") => arg.clone(),
        _ => default_pdf_output_path(),
    };

    Some(CliArgs {
        mode: CliMode::Img2Pdf,
        input_folder,
        output_path,
        canvas_width: parse_width(args),
        max_performance: parse_max_performance(args),
        output_format: OutputImageFormat::Jpeg,
        lossless: args.iter().any(|a| a == "--lossless"),
    })
}

fn parse_pdf2img_args(args: &[String]) -> Option<CliArgs> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_pdf2img_usage();
        std::process::exit(0);
    }

    if args.is_empty() {
        print_pdf2img_usage();
        std::process::exit(1);
    }

    let input_pdf = args[0].clone();
    let output_path = match args.get(1) {
        Some(arg) if !arg.starts_with("--") => arg.clone(),
        _ => default_image_output_folder(&input_pdf),
    };
    let output_format = args
        .windows(2)
        .find(|w| w[0] == "--format")
        .and_then(|w| OutputImageFormat::parse(&w[1]))
        .unwrap_or(OutputImageFormat::AutoLossless);

    Some(CliArgs {
        mode: CliMode::Pdf2Img,
        input_folder: input_pdf,
        output_path,
        canvas_width: parse_width(args),
        max_performance: parse_max_performance(args),
        output_format,
        lossless: false,
    })
}

fn parse_width(args: &[String]) -> u32 {
    args.windows(2)
        .find(|w| w[0] == "--width")
        .and_then(|w| w[1].parse::<u32>().ok())
        .unwrap_or(DEFAULT_WIDTH)
}

fn parse_max_performance(args: &[String]) -> bool {
    !args.iter().any(|a| a == "--no-max-performance")
}

/// ヘルプを表示する
fn print_usage() {
    eprintln!(
        "使用方法: img2pdf <input_folder> [output.pdf] [--width <px>] [--lossless] [--max-performance|--no-max-performance]\n\n引数:\n  <input_folder>       処理する JPEG 画像が入ったフォルダ\n  [output.pdf]         出力 PDF ファイルパス（省略時: output-[seed].pdf）\n\nオプション:\n  --width <px>         キャンバス幅（ピクセル）[デフォルト: {}]\n  --lossless           可能なら JPEG を再圧縮せずそのまま PDF に埋め込む\n  --max-performance   最大性能モード（全CPU使用）[デフォルト]\n  --no-max-performance 最大性能モードを無効化\n  --help, -h           このヘルプを表示\n\npdf2img:\n  img2pdf pdf2img <input.pdf> [output_folder] [--width <px>] [--format auto|jpg|png]\n\n引数なしで起動すると GUI モードで起動します。",
        DEFAULT_WIDTH
    );
}

/// pdf2img ヘルプを表示する
pub fn print_pdf2img_usage() {
    eprintln!(
        "使用方法: pdf2img <input.pdf> [output_folder] [--width <px>] [--format auto|jpg|png] [--max-performance|--no-max-performance]\n       img2pdf pdf2img <input.pdf> [output_folder] [--width <px>] [--format auto|jpg|png]\n\n引数:\n  <input.pdf>          入力 PDF ファイル\n  [output_folder]      出力画像フォルダ（省略時: <input>-images）\n\nオプション:\n  --width <px>         レンダリング時の出力画像幅（ピクセル）[デフォルト: {}]\n  --format auto|jpg|png 出力形式 [デフォルト: auto]\n                         auto は埋め込み JPEG を無劣化抽出し、不可なら PNG レンダリング\n  --max-performance   最大性能モード（全CPU使用）[デフォルト]\n  --no-max-performance 最大性能モードを無効化\n  --help, -h           このヘルプを表示",
        DEFAULT_WIDTH
    );
}

/// 既定の出力先 `output-[seed].pdf` を現在ディレクトリに生成する
fn default_pdf_output_path() -> String {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let file_name = format!("output-{seed}.pdf");
    PathBuf::from(file_name).to_string_lossy().to_string()
}

/// 既定の出力先 `<input>-images` を現在の入力 PDF と同じ階層に生成する
fn default_image_output_folder(input_pdf: &str) -> String {
    let input = Path::new(input_pdf);
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("output");
    let dir_name = format!("{stem}-images");
    input
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|parent| parent.join(&dir_name))
        .unwrap_or_else(|| PathBuf::from(dir_name))
        .to_string_lossy()
        .to_string()
}

/// CLI モードでの処理を実行する（ブロッキング）
pub fn run(args: &CliArgs) -> ProcessingResult {
    match args.mode {
        CliMode::Img2Pdf => run_img2pdf(args),
        CliMode::Pdf2Img => run_pdf2img(args),
    }
}

fn run_img2pdf(args: &CliArgs) -> ProcessingResult {
    ImageProcessor::set_max_performance_mode(args.max_performance);
    if args.max_performance {
        eprintln!("最大性能モード: ON");
    }

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
        eprintln!(
            "エラー: フォルダ内に JPEG ファイルが見つかりません: {}",
            args.input_folder
        );
        return ProcessingResult {
            success: false,
            success_count: 0,
            error_count: 1,
            errors: vec![ProcessingError {
                file_path: args.input_folder.clone(),
                message: "No JPEG files found".to_string(),
            }],
            output_path: args.output_path.clone(),
        };
    }

    eprintln!(
        "{} 枚の JPEG を検出しました。処理を開始します...",
        file_list.len()
    );
    run_with_progress(
        file_list.len(),
        "画像処理",
        "PDF保存",
        &args.output_path,
        |progress_tx, finished_tx| {
            ImageProcessor::run(
                file_list,
                args.canvas_width,
                args.output_path.clone(),
                args.lossless,
                Some(progress_tx),
                finished_tx,
            );
        },
    )
}

fn run_pdf2img(args: &CliArgs) -> ProcessingResult {
    ImageProcessor::set_max_performance_mode(args.max_performance);
    if args.max_performance {
        eprintln!("最大性能モード: ON");
    }

    eprintln!("PDF から画像への変換を開始します...");
    run_with_progress(
        0,
        "PDF変換",
        "画像保存",
        &args.output_path,
        |progress_tx, finished_tx| {
            Pdf2ImgProcessor::run(
                args.input_folder.clone(),
                args.output_path.clone(),
                args.canvas_width,
                args.output_format,
                Some(progress_tx),
                finished_tx,
            );
        },
    )
}

fn run_with_progress<F>(
    fallback_total: usize,
    processing_label: &'static str,
    saving_label: &'static str,
    output_path: &str,
    start: F,
) -> ProcessingResult
where
    F: FnOnce(mpsc::Sender<ProgressUpdate>, mpsc::Sender<ProcessingResult>),
{
    let (progress_tx, progress_rx) = mpsc::channel::<ProgressUpdate>();
    let (finished_tx, finished_rx) = mpsc::channel::<ProcessingResult>();

    let progress_handle = std::thread::spawn(move || {
        let mut has_output = false;
        for update in progress_rx {
            let (label, count, total) = match update.phase {
                ProgressPhase::Processing => (processing_label, update.count, update.total),
                ProgressPhase::Saving => {
                    let total = update.total.max(fallback_total);
                    (saving_label, update.count.max(total), total)
                }
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

    start(progress_tx, finished_tx);

    let result = finished_rx.recv().unwrap_or_else(|_| ProcessingResult {
        success: false,
        success_count: 0,
        error_count: 1,
        errors: vec![ProcessingError {
            file_path: "internal".to_string(),
            message: "Processing thread disconnected".to_string(),
        }],
        output_path: output_path.to_string(),
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
            if path.is_file()
                && let Some(ext) = path.extension()
            {
                let ext = ext.to_string_lossy().to_lowercase();
                if ext == "jpg" || ext == "jpeg" {
                    files.push(path.to_string_lossy().to_string());
                }
            }
        }
    }
    files
}
