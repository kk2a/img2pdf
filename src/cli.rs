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
    BookScanConfig, BookScanProcessor, BookScanProgress, JpegSampling, PageRange,
    SuperResolutionMode,
};
use crate::image_processor::ImageProcessor;
use crate::models::{ProcessingError, ProcessingResult, ProgressPhase, ProgressUpdate};
use crate::pdf2img_processor::{OutputImageFormat, Pdf2ImgProcessor};
use crate::utils::constants::DEFAULT_WIDTH;
use std::fmt::Write as FmtWrite;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, mpsc};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BookOptionSection {
    Page,
    Blank,
    ScanTailor,
    SuperResolution,
    ToneAndJpeg,
    Shortcut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BookOptionArity {
    Value,
    OptionalValue,
    Flag,
}

#[derive(Debug, Clone, Copy)]
struct BookOptionSpec {
    name: &'static str,
    value: &'static str,
    arity: BookOptionArity,
    section: BookOptionSection,
    description: &'static str,
    default: &'static str,
}

macro_rules! value_option {
    ($name:literal, $value:literal, $section:ident, $description:literal, $default:literal) => {
        BookOptionSpec {
            name: $name,
            value: $value,
            arity: BookOptionArity::Value,
            section: BookOptionSection::$section,
            description: $description,
            default: $default,
        }
    };
}

macro_rules! flag_option {
    ($name:literal, $section:ident, $description:literal) => {
        BookOptionSpec {
            name: $name,
            value: "",
            arity: BookOptionArity::Flag,
            section: BookOptionSection::$section,
            description: $description,
            default: "",
        }
    };
}

const BOOK_OPTIONS: &[BookOptionSpec] = &[
    value_option!(
        "--pages",
        "<N|START-END>",
        Page,
        "処理対象ページ",
        "全ページ"
    ),
    value_option!(
        "--work-dir",
        "<PATH>",
        Page,
        "作業フォルダ",
        "$TMPDIR/img2pdf-book-..."
    ),
    value_option!("--resume", "<on|off>", Page, "検証済み段階を再利用", "on"),
    BookOptionSpec {
        name: "--keep-work",
        value: "[on|off]",
        arity: BookOptionArity::OptionalValue,
        section: BookOptionSection::Page,
        description: "成功後も中間画像を保持",
        default: "off",
    },
    value_option!(
        "--blank-detection",
        "<on|off>",
        Blank,
        "保守的な空白判定",
        "on"
    ),
    value_option!(
        "--blank-dark-delta",
        "<1..255>",
        Blank,
        "背景から暗部とみなす差",
        "25"
    ),
    value_option!(
        "--blank-max-dark-ratio",
        "<0..0.1>",
        Blank,
        "空白とみなす最大暗部比率",
        "0.0005"
    ),
    value_option!(
        "--blank-edge-threshold",
        "<1..255>",
        Blank,
        "エッジの輝度差",
        "12"
    ),
    value_option!(
        "--blank-max-edge-ratio",
        "<0..0.1>",
        Blank,
        "空白とみなす最大エッジ比率",
        "0.0005"
    ),
    value_option!(
        "--scantailor",
        "<on|off>",
        ScanTailor,
        "ScanTailor全体",
        "on"
    ),
    value_option!("--crop", "<on|off>", ScanTailor, "ページ領域検出", "on"),
    value_option!(
        "--crop-exclude-pages",
        "<LIST>",
        ScanTailor,
        "cropしないページ（例: 1,10-12）",
        "1"
    ),
    value_option!(
        "--normalize",
        "<on|off>",
        ScanTailor,
        "背景照明正規化",
        "on"
    ),
    value_option!(
        "--color-normalize",
        "<on|off>",
        ScanTailor,
        "RGBのカラー紙面補正",
        "on"
    ),
    value_option!(
        "--color-normalize-strength",
        "<0..100>",
        ScanTailor,
        "カラー紙面補正の強さ",
        "100"
    ),
    value_option!(
        "--color-normalize-radius",
        "<1..256>",
        ScanTailor,
        "背景推定半径",
        "40"
    ),
    value_option!(
        "--ink-neutralize",
        "<on|off>",
        ScanTailor,
        "黒文字近傍だけ色差を除去",
        "off"
    ),
    value_option!(
        "--ink-neutralize-strength",
        "<0..100>",
        ScanTailor,
        "黒インク補正の強さ",
        "100"
    ),
    value_option!(
        "--ink-neutralize-exclude-pages",
        "<LIST>",
        ScanTailor,
        "黒インク補正をしないページ",
        "1"
    ),
    value_option!("--deskew", "<on|off>", ScanTailor, "傾き補正", "off"),
    value_option!("--dewarp", "<on|off>", ScanTailor, "湾曲補正", "off"),
    value_option!("--margins", "<NUMBER>", ScanTailor, "余白", "0"),
    value_option!("--dpi", "<NUMBER>", ScanTailor, "入力DPI", "300"),
    value_option!("--output-dpi", "<NUMBER>", ScanTailor, "出力DPI", "300"),
    value_option!("--despeckle", "<1.0..3.0>", ScanTailor, "ノイズ除去", "1.0"),
    value_option!(
        "--page-detection-tolerance",
        "<0..1>",
        ScanTailor,
        "ページ検出許容値",
        "0.1"
    ),
    value_option!(
        "--scantailor-path",
        "<PATH>",
        ScanTailor,
        "scantailor-cliの明示パス",
        "自動検出"
    ),
    value_option!(
        "--superres",
        "<off|anime|lanczos>",
        SuperResolution,
        "超解像方式",
        "anime"
    ),
    value_option!(
        "--output-scale",
        "<1..4>",
        SuperResolution,
        "最終画像の画素倍率",
        "1"
    ),
    value_option!(
        "--ai-scale",
        "<2..4>",
        SuperResolution,
        "Real-ESRGAN内部倍率",
        "2"
    ),
    value_option!(
        "--scale",
        "<1..4>",
        SuperResolution,
        "--output-scaleの旧名",
        "1"
    ),
    value_option!(
        "--inference-scale",
        "<2..4>",
        SuperResolution,
        "--ai-scaleの旧名",
        "2"
    ),
    value_option!(
        "--model",
        "<NAME>",
        SuperResolution,
        "Real-ESRGAN model",
        "realesr-animevideov3"
    ),
    value_option!(
        "--gpu-workers",
        "<1..8>",
        SuperResolution,
        "GPUプロセス数",
        "4"
    ),
    value_option!(
        "--cpu-workers",
        "<1..64>",
        SuperResolution,
        "CPU後処理worker数",
        "2"
    ),
    value_option!(
        "--tile",
        "<NUMBER>",
        SuperResolution,
        "ncnn tile size（0はauto）",
        "0"
    ),
    value_option!(
        "--gpu-id",
        "<NUMBER>",
        SuperResolution,
        "GPU ID（-1はauto）",
        "-1"
    ),
    value_option!("--tta", "<on|off>", SuperResolution, "TTA", "off"),
    value_option!(
        "--realesrgan-path",
        "<PATH>",
        SuperResolution,
        "realesrgan-ncnn-vulkanの明示パス",
        "自動検出"
    ),
    value_option!(
        "--model-dir",
        "<PATH>",
        SuperResolution,
        "modelフォルダ",
        "実行ファイル隣接"
    ),
    value_option!(
        "--pre-stroke",
        "<on|off>",
        SuperResolution,
        "超解像前の線補強",
        "off"
    ),
    value_option!(
        "--pre-stroke-strength",
        "<0..100>",
        SuperResolution,
        "超解像前Minimum blend率",
        "5"
    ),
    value_option!(
        "--tone-boost",
        "<on|off>",
        ToneAndJpeg,
        "紙・インク基準の階調補正",
        "on"
    ),
    value_option!(
        "--tone-boost-strength",
        "<0..100>",
        ToneAndJpeg,
        "階調補正の強さ",
        "100"
    ),
    value_option!("--stroke", "<on|off>", ToneAndJpeg, "文字太さ調整", "off"),
    value_option!(
        "--stroke-strength",
        "<0..100>",
        ToneAndJpeg,
        "Minimum blend率",
        "15"
    ),
    value_option!(
        "--tone-color-global-threshold",
        "<0..1>",
        ToneAndJpeg,
        "tone boost用の全体色面積閾値",
        "0.01"
    ),
    value_option!(
        "--tone-color-tile-threshold",
        "<0..1>",
        ToneAndJpeg,
        "tone boost用の局所色面積閾値",
        "0.30"
    ),
    value_option!(
        "--tone-exclude-pages",
        "<LIST>",
        ToneAndJpeg,
        "tone boostを適用しないページ",
        "1"
    ),
    value_option!(
        "--grayscale-pages",
        "<LIST>",
        ToneAndJpeg,
        "指定ページだけ全体を1成分Gray化",
        "無効"
    ),
    value_option!("--jpeg-quality", "<1..100>", ToneAndJpeg, "JPEG品質", "90"),
    value_option!(
        "--jpeg-sampling",
        "<444|422|420>",
        ToneAndJpeg,
        "chroma sampling",
        "444"
    ),
    value_option!(
        "--preserve-position",
        "<on|off>",
        ToneAndJpeg,
        "A4上で元位置・等倍比を保持",
        "on"
    ),
    flag_option!("--no-scantailor", Shortcut, "ScanTailor全体を無効化"),
    flag_option!("--no-blank-detection", Shortcut, "空白判定を無効化"),
    flag_option!("--no-crop", Shortcut, "cropを無効化"),
    flag_option!("--no-normalize", Shortcut, "背景照明正規化を無効化"),
    flag_option!("--no-color-normalize", Shortcut, "カラー紙面補正を無効化"),
    flag_option!("--no-ink-neutralize", Shortcut, "黒インク補正を無効化"),
    flag_option!("--no-tone-boost", Shortcut, "tone boostを無効化"),
    flag_option!("--no-stroke", Shortcut, "文字太さ調整を無効化"),
    flag_option!("--no-resume", Shortcut, "中断再開を無効化"),
    flag_option!("--max-performance", Shortcut, "最大性能モード"),
    flag_option!("--no-max-performance", Shortcut, "最大性能モードを無効化"),
];

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
                    Err(_) => errors.push(book_option_value_error($name, value)),
                }
            }
        };
    }
    macro_rules! toggle {
        ($name:literal, $field:ident) => {
            if let Some(value) = option_value(args, $name) {
                match parse_on_off(value) {
                    Some(value) => config.$field = value,
                    None => errors.push(book_option_value_error($name, value)),
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
    toggle!("--tone-boost", tone_boost_enabled);
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
    number!("--tone-boost-strength", tone_boost_strength, u8);
    number!(
        "--tone-color-global-threshold",
        tone_color_global_threshold,
        f32
    );
    number!(
        "--tone-color-tile-threshold",
        tone_color_tile_threshold,
        f32
    );
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
    if let Some(value) = option_value(args, "--tone-exclude-pages") {
        match PageRange::parse_list(value) {
            Some(value) => config.tone_exclude_pages = value,
            None => errors.push(format!("--tone-exclude-pages の値が不正です: {value}")),
        }
    }
    if let Some(value) = option_value(args, "--grayscale-pages") {
        match PageRange::parse_list(value) {
            Some(value) => config.grayscale_pages = value,
            None => errors.push(format!("--grayscale-pages の値が不正です: {value}")),
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
    if args.iter().any(|arg| arg == "--no-tone-boost") {
        config.tone_boost_enabled = false;
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

fn book_option_value_error(name: &str, value: &str) -> String {
    let expected = BOOK_OPTIONS
        .iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.value)
        .filter(|expected| !expected.is_empty())
        .unwrap_or("有効な値");
    format!("{name} の値が不正です: {value}（期待: {expected}）")
}

fn parse_on_off(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn validate_book_option_names(args: &[String], errors: &mut Vec<String>) {
    for (index, argument) in args.iter().enumerate() {
        if !argument.starts_with("--") {
            continue;
        }
        let name = argument
            .split_once('=')
            .map_or(argument.as_str(), |pair| pair.0);
        let Some(spec) = BOOK_OPTIONS.iter().find(|spec| spec.name == name) else {
            errors.push(format!("不明なオプションです: {name}"));
            continue;
        };
        if spec.arity == BookOptionArity::Flag && argument.contains('=') {
            errors.push(format!("{name} は値を取りません"));
        }
        if spec.arity == BookOptionArity::Value
            && !argument.contains('=')
            && args
                .get(index + 1)
                .is_none_or(|value| value.starts_with("--"))
        {
            errors.push(format!("{name} に値がありません（期待: {}）", spec.value));
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
    eprint!("{}", book_scan_usage());
}

/// オプション仕様表から本モードhelpを生成する。
pub fn book_scan_usage() -> String {
    let mut output = String::from(
        "使用方法:\n  img2pdf book-scan <input.pdf|image_folder> [output.pdf] [options]\n\n\
既定プリセット:\n  ScanTailor/crop/背景正規化/tone boost ON、deskew/dewarp/stroke OFF\n  Anime AI内部x2 → 最終x1、JPEG Q90 4:4:4、A4上で元位置と縦横比を保持\n  自動グレースケールは無効。1成分Grayは --grayscale-pages で明示したページだけ\n  表紙はcrop除外、空白ページは元画像を埋め込み\n",
    );
    let sections = [
        (BookOptionSection::Page, "ページ・作業"),
        (BookOptionSection::Blank, "空白ページ"),
        (BookOptionSection::ScanTailor, "ScanTailor前処理"),
        (BookOptionSection::SuperResolution, "超解像"),
        (BookOptionSection::ToneAndJpeg, "文字・階調・JPEG"),
        (BookOptionSection::Shortcut, "短縮flag"),
    ];
    for (section, title) in sections {
        let _ = writeln!(output, "\n{title}:");
        for spec in BOOK_OPTIONS.iter().filter(|spec| spec.section == section) {
            let signature = if spec.value.is_empty() {
                spec.name.to_string()
            } else {
                format!("{} {}", spec.name, spec.value)
            };
            let default = if spec.default.is_empty() {
                String::new()
            } else {
                format!(" [{}]", spec.default)
            };
            let _ = writeln!(output, "  {signature:<42} {}{default}", spec.description);
        }
    }
    output.push_str(
        "\nページLIST形式:\n  1,9,20-22 のように1始まりで指定。0、逆順、空要素はエラー\n\n\
環境変数:\n  IMG2PDF_SCANTAILOR / IMG2PDF_REALESRGAN\n",
    );
    output
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
    let display = Mutex::new(CliBookProgress::new());
    let result = BookScanProcessor::run_with_progress(config, |update| {
        if let Ok(mut display) = display.lock() {
            display.render(&update);
        }
    });
    if let Ok(mut display) = display.lock() {
        display.finish(result.is_ok());
    }
    match result {
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

struct CliBookProgress {
    interactive: bool,
    last_stage: Option<usize>,
}

impl CliBookProgress {
    const BAR_WIDTH: usize = 24;

    fn new() -> Self {
        Self {
            interactive: io::stderr().is_terminal(),
            last_stage: None,
        }
    }

    fn render(&mut self, update: &BookScanProgress) {
        if self.interactive {
            let within_stage = match (update.completed, update.total) {
                (Some(completed), Some(total)) if total > 0 => {
                    completed.min(total) as f32 / total as f32
                }
                _ => 0.0,
            };
            let overall = ((update.stage.saturating_sub(1)) as f32 + within_stage)
                / update.total_stages.max(1) as f32;
            let filled = (overall * Self::BAR_WIDTH as f32).round() as usize;
            let bar = format!(
                "{}{}",
                "=".repeat(filled.min(Self::BAR_WIDTH)),
                " ".repeat(Self::BAR_WIDTH.saturating_sub(filled))
            );
            eprint!(
                "\r\x1b[2K本モード [{bar}] {:>3}% [{}/{}] {}",
                (overall * 100.0).round() as usize,
                update.stage,
                update.total_stages,
                update.message
            );
            let _ = io::stderr().flush();
        } else if self.last_stage != Some(update.stage) {
            eprintln!(
                "本モード [{}/{}]: {}",
                update.stage, update.total_stages, update.message
            );
        }
        self.last_stage = Some(update.stage);
    }

    fn finish(&mut self, success: bool) {
        if !self.interactive {
            return;
        }
        if success {
            eprint!(
                "\r\x1b[2K本モード [{}] 100% 完了\n",
                "=".repeat(Self::BAR_WIDTH)
            );
        } else {
            eprint!("\n");
        }
        let _ = io::stderr().flush();
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
