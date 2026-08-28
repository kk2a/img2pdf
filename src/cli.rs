//! CLI（ヘッドレス）モード
//!
//! ディスプレイなしでも動作する。コマンドライン引数を解析して
//! ImageProcessor / Pdf2ImgProcessor を直接呼び出す。
//!
//! ## 使用方法
//! ```text
//! img2pdf <input_folder> [output.pdf] [options]
//! img2pdf pdf2img <input.pdf> [output_folder] [options]
//! img2pdf book-scan <input.pdf|image_folder> [output.pdf] [options]
//! pdf2img <input.pdf> [output_folder] [options]
//! ```

use crate::book_scan::{
    BookScanConfig, BookScanProcessor, JpegSampling, PageRange, SuperResolutionMode,
};
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
    BookScan,
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
    pub book_scan: Option<BookScanConfig>,
    pub parse_error: Option<String>,
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

    if args[0] == "book-scan" {
        return parse_book_scan_args(&args[1..]);
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
        book_scan: None,
        parse_error: None,
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
        book_scan: None,
        parse_error: None,
    })
}

fn parse_book_scan_args(args: &[String]) -> Option<CliArgs> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_book_scan_usage();
        std::process::exit(0);
    }
    if args.is_empty() {
        print_book_scan_usage();
        std::process::exit(1);
    }

    let input = PathBuf::from(&args[0]);
    let output = match args.get(1) {
        Some(value) if !value.starts_with("--") => PathBuf::from(value),
        _ => default_book_output_path(&input),
    };
    let mut config = BookScanConfig {
        input_path: input,
        output_pdf: output,
        ..BookScanConfig::default()
    };
    let mut errors = Vec::new();
    validate_book_option_names(args, &mut errors);

    macro_rules! number {
        ($name:literal, $field:ident, $type:ty) => {
            if let Some(value) = option_value(args, $name) {
                match value.parse::<$type>() {
                    Ok(value) => config.$field = value,
                    Err(_) => errors.push(format!("{} の値が不正です: {}", $name, value)),
                }
            }
        };
    }
    macro_rules! toggle {
        ($name:literal, $field:ident) => {
            if let Some(value) = option_value(args, $name) {
                match parse_on_off(value) {
                    Some(value) => config.$field = value,
                    None => errors.push(format!("{} にはon/offを指定してください: {}", $name, value)),
                }
            }
        };
    }

    toggle!("--scantailor", scantailor_enabled);
    toggle!("--blank-detection", blank_detection_enabled);
    toggle!("--crop", crop_enabled);
    toggle!("--normalize", normalize_illumination);
    toggle!("--color-normalize", color_normalization_enabled);
    toggle!("--ink-neutralize", ink_neutralization_enabled);
    toggle!("--deskew", deskew_enabled);
    toggle!("--dewarp", dewarp_enabled);
    toggle!("--tta", tta_enabled);
    toggle!("--stroke", stroke_enabled);
    toggle!("--pre-stroke", pre_stroke_enabled);
    toggle!("--preserve-position", preserve_position);
    toggle!("--resume", resume);
    toggle!("--keep-work", keep_work);
    number!("--margins", margins, f32);
    number!("--blank-dark-delta", blank_dark_delta, u8);
    number!("--blank-max-dark-ratio", blank_max_dark_ratio, f32);
    number!("--blank-edge-threshold", blank_edge_threshold, u8);
    number!("--blank-max-edge-ratio", blank_max_edge_ratio, f32);
    number!("--dpi", dpi, u32);
    number!("--output-dpi", output_dpi, u32);
    number!("--despeckle", despeckle, f32);
    number!("--page-detection-tolerance", page_detection_tolerance, f32);
    number!(
        "--color-normalize-strength",
        color_normalization_strength,
        u8
    );
    number!("--color-normalize-radius", color_normalization_radius, u32);
    number!("--ink-neutralize-strength", ink_neutralization_strength, u8);
    // 旧名を先に読み、新しい明示的な名前が両方ある場合は新名を優先する。
    number!("--scale", superres_output_scale, u32);
    number!("--inference-scale", superres_ai_scale, u32);
    number!("--output-scale", superres_output_scale, u32);
    number!("--ai-scale", superres_ai_scale, u32);
    number!("--gpu-workers", gpu_workers, usize);
    number!("--cpu-workers", cpu_workers, usize);
    number!("--tile", tile_size, u32);
    number!("--gpu-id", gpu_id, i32);
    number!("--stroke-strength", stroke_strength, u8);
    number!("--pre-stroke-strength", pre_stroke_strength, u8);
    number!("--jpeg-quality", jpeg_quality, u8);

    if let Some(value) = option_value(args, "--pages") {
        match PageRange::parse(value) {
            Some(value) => config.pages = Some(value),
            None => errors.push(format!("--pages の値が不正です: {value}")),
        }
    }
    if let Some(value) = option_value(args, "--crop-exclude-pages") {
        match PageRange::parse_list(value) {
            Some(value) => config.crop_exclude_pages = value,
            None => errors.push(format!("--crop-exclude-pages の値が不正です: {value}")),
        }
    }
    if let Some(value) = option_value(args, "--ink-neutralize-exclude-pages") {
        match PageRange::parse_list(value) {
            Some(value) => config.ink_neutralization_exclude_pages = value,
            None => errors.push(format!(
                "--ink-neutralize-exclude-pages の値が不正です: {value}"
            )),
        }
    }
    if let Some(value) = option_value(args, "--superres") {
        match SuperResolutionMode::parse(value) {
            Some(value) => config.super_resolution = value,
            None => errors.push(format!("--superres の値が不正です: {value}")),
        }
    }
    if let Some(value) = option_value(args, "--jpeg-sampling") {
        match JpegSampling::parse(value) {
            Some(value) => config.jpeg_sampling = value,
            None => errors.push(format!("--jpeg-sampling の値が不正です: {value}")),
        }
    }
    if let Some(value) = option_value(args, "--model") {
        config.superres_model = value.to_string();
    }
    if let Some(value) = option_value(args, "--work-dir") {
        config.work_dir = Some(PathBuf::from(value));
    }
    if let Some(value) = option_value(args, "--scantailor-path") {
        config.scantailor_path = Some(PathBuf::from(value));
    }
    if let Some(value) = option_value(args, "--realesrgan-path") {
        config.realesrgan_path = Some(PathBuf::from(value));
    }
    if let Some(value) = option_value(args, "--model-dir") {
        config.realesrgan_model_dir = Some(PathBuf::from(value));
    }

    // 値を取らない短縮flagも用意する。
    if args.iter().any(|arg| arg == "--no-scantailor") {
        config.scantailor_enabled = false;
    }
    if args.iter().any(|arg| arg == "--no-blank-detection") {
        config.blank_detection_enabled = false;
    }
    if args.iter().any(|arg| arg == "--no-crop") {
        config.crop_enabled = false;
    }
    if args.iter().any(|arg| arg == "--no-normalize") {
        config.normalize_illumination = false;
    }
    if args.iter().any(|arg| arg == "--no-color-normalize") {
        config.color_normalization_enabled = false;
    }
    if args.iter().any(|arg| arg == "--no-ink-neutralize") {
        config.ink_neutralization_enabled = false;
    }
    if args.iter().any(|arg| arg == "--no-stroke") {
        config.stroke_enabled = false;
    }
    if args.iter().any(|arg| arg == "--no-resume") {
        config.resume = false;
    }
    if args.iter().any(|arg| arg == "--keep-work") && option_value(args, "--keep-work").is_none() {
        config.keep_work = true;
    }
    if !config.scantailor_enabled {
        config.crop_enabled = false;
        config.normalize_illumination = false;
        config.deskew_enabled = false;
        config.dewarp_enabled = false;
    }

    Some(CliArgs {
        mode: CliMode::BookScan,
        input_folder: config.input_path.to_string_lossy().to_string(),
        output_path: config.output_pdf.to_string_lossy().to_string(),
        canvas_width: DEFAULT_WIDTH,
        max_performance: parse_max_performance(args),
        output_format: OutputImageFormat::Jpeg,
        lossless: false,
        book_scan: Some(config),
        parse_error: (!errors.is_empty()).then(|| errors.join("; ")),
    })
}

fn option_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == name && !window[1].starts_with("--"))
        .map(|window| window[1].as_str())
        .or_else(|| {
            let prefix = format!("{name}=");
            args.iter().find_map(|arg| arg.strip_prefix(&prefix))
        })
}

fn parse_on_off(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn validate_book_option_names(args: &[String], errors: &mut Vec<String>) {
    const VALUE_OPTIONS: &[&str] = &[
        "--pages",
        "--work-dir",
        "--resume",
        "--blank-detection",
        "--blank-dark-delta",
        "--blank-max-dark-ratio",
        "--blank-edge-threshold",
        "--blank-max-edge-ratio",
        "--scantailor",
        "--crop",
        "--crop-exclude-pages",
        "--normalize",
        "--color-normalize",
        "--color-normalize-strength",
        "--color-normalize-radius",
        "--ink-neutralize",
        "--ink-neutralize-strength",
        "--ink-neutralize-exclude-pages",
        "--deskew",
        "--dewarp",
        "--margins",
        "--dpi",
        "--output-dpi",
        "--despeckle",
        "--page-detection-tolerance",
        "--scantailor-path",
        "--superres",
        "--scale",
        "--inference-scale",
        "--output-scale",
        "--ai-scale",
        "--model",
        "--gpu-workers",
        "--cpu-workers",
        "--tile",
        "--gpu-id",
        "--tta",
        "--realesrgan-path",
        "--model-dir",
        "--stroke",
        "--stroke-strength",
        "--pre-stroke",
        "--pre-stroke-strength",
        "--jpeg-quality",
        "--jpeg-sampling",
        "--preserve-position",
    ];
    const FLAGS: &[&str] = &[
        "--keep-work",
        "--no-scantailor",
        "--no-blank-detection",
        "--no-crop",
        "--no-normalize",
        "--no-color-normalize",
        "--no-ink-neutralize",
        "--no-stroke",
        "--no-resume",
        "--max-performance",
        "--no-max-performance",
    ];

    for (index, argument) in args.iter().enumerate() {
        if !argument.starts_with("--") {
            continue;
        }
        let name = argument
            .split_once('=')
            .map_or(argument.as_str(), |pair| pair.0);
        if !VALUE_OPTIONS.contains(&name) && !FLAGS.contains(&name) {
            errors.push(format!("不明なオプションです: {name}"));
            continue;
        }
        if VALUE_OPTIONS.contains(&name)
            && !argument.contains('=')
            && args
                .get(index + 1)
                .is_none_or(|value| value.starts_with("--"))
        {
            errors.push(format!("{name} に値がありません"));
        }
    }
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
        "使用方法: img2pdf <input_folder> [output.pdf] [--width <px>] [--lossless] [--max-performance|--no-max-performance]\n\n引数:\n  <input_folder>       処理する JPEG 画像が入ったフォルダ\n  [output.pdf]         出力 PDF ファイルパス（省略時: output-[seed].pdf）\n\nオプション:\n  --width <px>         キャンバス幅（ピクセル）[デフォルト: {}]\n  --lossless           可能なら JPEG を再圧縮せずそのまま PDF に埋め込む\n  --max-performance   最大性能モード（全CPU使用）[デフォルト]\n  --no-max-performance 最大性能モードを無効化\n  --help, -h           このヘルプを表示\n\npdf2img:\n  img2pdf pdf2img <input.pdf> [output_folder] [--width <px>] [--format auto|jpg|png]\n\n本モード:\n  img2pdf book-scan <input.pdf|image_folder> [output.pdf] [options]\n  img2pdf book-scan --help\n\n引数なしで起動すると GUI モードで起動します。",
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

/// 本モードのヘルプを表示する。
pub fn print_book_scan_usage() {
    eprintln!(
        r#"使用方法:
  img2pdf book-scan <input.pdf|image_folder> [output.pdf] [options]

既定プリセット:
  ScanTailor ON / crop ON / 背景正規化 ON / deskew OFF / dewarp OFF
  カラー紙面補正 ON / 黒インク色差補正 OFF
  animevideov3でAI内部x2、最終画像x1 / GPU worker 4 / Minimum 15
  JPEG Q90 4:4:4 / A4上の元位置・サイズを保持
  表紙（1ページ目）はcrop除外 / 空白ページの重い処理を省略して元画像を埋め込み

ページ・作業:
  --pages <N|START-END>          対象ページ
  --work-dir <path>              作業フォルダ
  --resume <on|off>              検証済み段階を再利用 [on]
  --keep-work [on|off]           成功後も中間画像を保持 [off]

空白ページ:
  --blank-detection <on|off>     保守的な空白判定 [on]
  --blank-dark-delta <1..255>    背景から暗部とみなす差 [25]
  --blank-max-dark-ratio <0..0.1> 空白とみなす最大暗部比率 [0.0005]
  --blank-edge-threshold <1..255> エッジの輝度差 [12]
  --blank-max-edge-ratio <0..0.1> 空白とみなす最大エッジ比率 [0.0005]

ScanTailor前処理:
  --scantailor <on|off>          ScanTailor全体 [on]
  --crop <on|off>                ページ領域検出 [on]
  --crop-exclude-pages <LIST>    cropしないページ（例: 1,158,10-12）[1]
  --normalize <on|off>           背景照明正規化 [on]
  --color-normalize <on|off>     RGBのカラー紙面補正 [on]
  --color-normalize-strength <0..100> 紙面補正の強さ [100]
  --color-normalize-radius <1..256> 背景推定半径 [40]
  --ink-neutralize <on|off>      黒文字近傍だけ色差を除去 [off]
  --ink-neutralize-strength <0..100> 黒インク補正の強さ [100]
  --ink-neutralize-exclude-pages <LIST> 補正しないページ [1]
  --deskew <on|off>              傾き補正 [off]
  --dewarp <on|off>              湾曲補正 [off]
  --margins <number>             余白 [0]
  --dpi <number>                 入力DPI [300]
  --output-dpi <number>          出力DPI [300]
  --despeckle <1.0..3.0>         ノイズ除去 [1.0]
  --page-detection-tolerance <0..1> ページ検出許容値 [0.1]
  --scantailor-path <path>       scantailor-cliの明示パス

超解像:
  --superres <off|anime|lanczos> 方式 [anime]
  --output-scale <1..4>          最終画像の画素倍率 [1]
  --ai-scale <2..4>              Real-ESRGAN内部倍率 [2]
  --scale / --inference-scale    上記2項目の旧名（互換用）
  --model <name>                 Real-ESRGAN model [realesr-animevideov3]
  --gpu-workers <1..8>           GPUプロセス数 [4]
  --cpu-workers <1..64>          CPU後処理worker数 [2]
  --tile <number>                ncnn tile size、0はauto [0]
  --gpu-id <number>              GPU ID、-1はauto [-1]
  --tta <on|off>                 TTA [off]
  --realesrgan-path <path>       realesrgan-ncnn-vulkanの明示パス
  --model-dir <path>             modelフォルダ
  --pre-stroke <on|off>          超解像前の線補強 [off]
  --pre-stroke-strength <0..100> 超解像前Minimum blend率 [5]

文字・JPEG:
  --stroke <on|off>              文字太さ調整 [on]
  --stroke-strength <0..100>     Minimum blend率 [15]
  --jpeg-quality <1..100>        JPEG品質 [90]
  --jpeg-sampling <444|422|420>  chroma sampling [444]
  --preserve-position <on|off>   A4上の元位置・サイズ保持 [on]

短縮flag:
  --no-scantailor --no-blank-detection --no-crop --no-normalize
  --no-color-normalize --no-ink-neutralize --no-stroke --no-resume

環境変数:
  IMG2PDF_SCANTAILOR / IMG2PDF_REALESRGAN
"#
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

fn default_book_output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("book");
    input
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}-book.pdf"))
}

/// CLI モードでの処理を実行する（ブロッキング）
pub fn run(args: &CliArgs) -> ProcessingResult {
    if let Some(error) = &args.parse_error {
        return failed_result(&args.output_path, "arguments", error);
    }
    match args.mode {
        CliMode::Img2Pdf => run_img2pdf(args),
        CliMode::Pdf2Img => run_pdf2img(args),
        CliMode::BookScan => run_book_scan(args),
    }
}

fn run_book_scan(args: &CliArgs) -> ProcessingResult {
    let Some(config) = args.book_scan.clone() else {
        return failed_result(&args.output_path, "book-scan", "本モード設定がありません");
    };
    match BookScanProcessor::run(config) {
        Ok(report) => {
            eprintln!(
                "本モード完了: {}ページ（空白{}ページ）、作業フォルダ: {}、{:.2}秒",
                report.page_count,
                report.blank_page_count,
                report.work_dir.display(),
                report.elapsed.as_secs_f64()
            );
            ProcessingResult {
                success: true,
                success_count: report.page_count,
                error_count: 0,
                errors: Vec::new(),
                output_path: report.output_pdf.to_string_lossy().to_string(),
            }
        }
        Err(error) => failed_result(&args.output_path, &args.input_folder, &error),
    }
}

fn failed_result(output_path: &str, file_path: &str, message: &str) -> ProcessingResult {
    ProcessingResult {
        success: false,
        success_count: 0,
        error_count: 1,
        errors: vec![ProcessingError {
            file_path: file_path.to_string(),
            message: message.to_string(),
        }],
        output_path: output_path.to_string(),
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
