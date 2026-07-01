use crate::models::{ProcessingError, ProcessingResult, ProgressPhase, ProgressUpdate};
use crate::utils::constants::PDF_QUALITY;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};

const PROJECT_TOOLS_DIR: &str = "tools";
const PDFTOPPM_TOOL_NAMES: &[&str] = if cfg!(windows) {
    &["pdftoppm.exe", "pdftoppm"]
} else {
    &["pdftoppm", "pdftoppm.exe"]
};
const PDFINFO_TOOL_NAMES: &[&str] = if cfg!(windows) {
    &["pdfinfo.exe", "pdfinfo"]
} else {
    &["pdfinfo", "pdfinfo.exe"]
};
const PDFIMAGES_TOOL_NAMES: &[&str] = if cfg!(windows) {
    &["pdfimages.exe", "pdfimages"]
} else {
    &["pdfimages", "pdfimages.exe"]
};

/// pdf2img の出力画像形式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputImageFormat {
    /// JPEG は無劣化抽出し、それ以外は PNG でレンダリングする。
    AutoLossless,
    Jpeg,
    Png,
}

impl OutputImageFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "auto" | "lossless" => Some(Self::AutoLossless),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::AutoLossless | Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    fn pdftoppm_flag(self) -> &'static str {
        match self {
            Self::AutoLossless | Self::Png => "-png",
            Self::Jpeg => "-jpeg",
        }
    }
}

#[derive(Debug, Clone)]
struct PdfImageInfo {
    page: usize,
    enc: String,
}

/// PDF から画像への変換を担うコアモジュール
pub struct Pdf2ImgProcessor;

impl Pdf2ImgProcessor {
    /// 利用可能な pdftoppm を探す。
    pub fn find_pdftoppm() -> Option<PathBuf> {
        static PDFTOPPM_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
        PDFTOPPM_PATH
            .get_or_init(|| find_tool(PDFTOPPM_TOOL_NAMES))
            .clone()
    }

    /// 利用可能な pdfinfo を探す。
    pub fn find_pdfinfo() -> Option<PathBuf> {
        static PDFINFO_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
        PDFINFO_PATH
            .get_or_init(|| find_tool(PDFINFO_TOOL_NAMES))
            .clone()
    }

    /// 利用可能な pdfimages を探す。
    pub fn find_pdfimages() -> Option<PathBuf> {
        static PDFIMAGES_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
        PDFIMAGES_PATH
            .get_or_init(|| find_tool(PDFIMAGES_TOOL_NAMES))
            .clone()
    }

    /// PDF のページ数を取得する。
    pub fn page_count(input_pdf: &Path) -> Result<usize, ProcessingError> {
        let Some(pdfinfo_path) = Self::find_pdfinfo() else {
            return Err(ProcessingError {
                file_path: input_pdf.to_string_lossy().to_string(),
                message: "pdfinfo が見つかりません。Poppler をインストールするか tools/ に配置してください".to_string(),
            });
        };

        let output = Command::new(pdfinfo_path)
            .arg(input_pdf)
            .output()
            .map_err(|e| ProcessingError {
                file_path: input_pdf.to_string_lossy().to_string(),
                message: format!("pdfinfo の実行に失敗しました: {e}"),
            })?;

        if !output.status.success() {
            return Err(ProcessingError {
                file_path: input_pdf.to_string_lossy().to_string(),
                message: format!(
                    "pdfinfo が失敗しました: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("Pages:") {
                let count = rest.trim().parse::<usize>().map_err(|e| ProcessingError {
                    file_path: input_pdf.to_string_lossy().to_string(),
                    message: format!("ページ数の解析に失敗しました: {e}"),
                })?;
                if count > 0 {
                    return Ok(count);
                }
            }
        }

        Err(ProcessingError {
            file_path: input_pdf.to_string_lossy().to_string(),
            message: "pdfinfo の出力からページ数を取得できませんでした".to_string(),
        })
    }

    /// 複数ページを並列変換する（別スレッドで実行）
    pub fn run(
        input_pdf: String,
        output_folder: String,
        canvas_width: u32,
        format: OutputImageFormat,
        progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
        finished_tx: mpsc::Sender<ProcessingResult>,
    ) {
        std::thread::spawn(move || {
            let result =
                Self::run_thread(input_pdf, output_folder, canvas_width, format, progress_tx);
            let _ = finished_tx.send(result);
        });
    }

    fn run_thread(
        input_pdf: String,
        output_folder: String,
        canvas_width: u32,
        format: OutputImageFormat,
        progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
    ) -> ProcessingResult {
        let input_path = PathBuf::from(&input_pdf);
        let output_dir = PathBuf::from(&output_folder);
        let mut errors = Vec::new();

        if canvas_width == 0 {
            return ProcessingResult {
                success: false,
                success_count: 0,
                error_count: 1,
                errors: vec![ProcessingError {
                    file_path: input_pdf,
                    message: "--width には 1 以上を指定してください".to_string(),
                }],
                output_path: output_folder,
            };
        }

        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            return ProcessingResult {
                success: false,
                success_count: 0,
                error_count: 1,
                errors: vec![ProcessingError {
                    file_path: output_folder.clone(),
                    message: format!("出力フォルダを作成できません: {e}"),
                }],
                output_path: output_folder,
            };
        }

        let total = match Self::page_count(&input_path) {
            Ok(total) => total,
            Err(e) => {
                return ProcessingResult {
                    success: false,
                    success_count: 0,
                    error_count: 1,
                    errors: vec![e],
                    output_path: output_folder,
                };
            }
        };

        let output_stem = input_path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("page")
            .to_string();

        if matches!(
            format,
            OutputImageFormat::AutoLossless | OutputImageFormat::Jpeg
        ) && let Ok(Some(success_count)) = Self::try_extract_lossless_jpegs(
            &input_path,
            &output_dir,
            &output_stem,
            total,
            progress_tx.as_ref(),
        ) {
            if let Some(ref tx) = progress_tx {
                let _ = tx.send(ProgressUpdate {
                    count: success_count,
                    total,
                    phase: ProgressPhase::Saving,
                });
            }
            return ProcessingResult {
                success: success_count > 0,
                success_count,
                error_count: 0,
                errors,
                output_path: output_folder,
            };
        }

        let render_format = if format == OutputImageFormat::AutoLossless {
            OutputImageFormat::Png
        } else {
            format
        };
        let counter = Arc::new(AtomicUsize::new(0));

        let mut results: Vec<(usize, Result<PathBuf, ProcessingError>)> = (1..=total)
            .into_par_iter()
            .map(|page| {
                let result = Self::render_page(
                    &input_path,
                    &output_dir,
                    &output_stem,
                    page,
                    canvas_width,
                    render_format,
                );

                if let Some(ref tx) = progress_tx {
                    let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                    let _ = tx.send(ProgressUpdate {
                        count,
                        total,
                        phase: ProgressPhase::Processing,
                    });
                }

                (page, result)
            })
            .collect();

        results.sort_unstable_by_key(|(page, _)| *page);

        let mut success_count = 0usize;
        for (_, result) in results {
            match result {
                Ok(_) => success_count += 1,
                Err(e) => errors.push(e),
            }
        }

        if let Some(ref tx) = progress_tx {
            let _ = tx.send(ProgressUpdate {
                count: success_count,
                total,
                phase: ProgressPhase::Saving,
            });
        }

        ProcessingResult {
            success: success_count > 0,
            success_count,
            error_count: errors.len(),
            errors,
            output_path: output_folder,
        }
    }

    fn try_extract_lossless_jpegs(
        input_pdf: &Path,
        output_dir: &Path,
        output_stem: &str,
        total: usize,
        progress_tx: Option<&mpsc::Sender<ProgressUpdate>>,
    ) -> Result<Option<usize>, ProcessingError> {
        let Some(pdfimages_path) = Self::find_pdfimages() else {
            return Ok(None);
        };

        let images = Self::list_pdf_images(&pdfimages_path, input_pdf)?;
        if !Self::can_losslessly_extract_jpegs(&images, total) {
            return Ok(None);
        }

        let tmp_dir = output_dir.join(format!(
            ".{output_stem}-pdfimages-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&tmp_dir).map_err(|e| ProcessingError {
            file_path: tmp_dir.to_string_lossy().to_string(),
            message: format!("一時フォルダを作成できません: {e}"),
        })?;

        let prefix = tmp_dir.join("page");
        let output = Command::new(&pdfimages_path)
            .arg("-j")
            .arg("-p")
            .arg(input_pdf)
            .arg(&prefix)
            .output()
            .map_err(|e| ProcessingError {
                file_path: input_pdf.to_string_lossy().to_string(),
                message: format!("pdfimages の実行に失敗しました: {e}"),
            })?;

        if !output.status.success() {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Ok(None);
        }

        let mut extracted = std::fs::read_dir(&tmp_dir)
            .map_err(|e| ProcessingError {
                file_path: tmp_dir.to_string_lossy().to_string(),
                message: format!("抽出画像の一覧取得に失敗しました: {e}"),
            })?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .and_then(|e| e.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg"))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        extracted.sort();

        if extracted.len() != total {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Ok(None);
        }

        for (idx, source) in extracted.iter().enumerate() {
            let output = output_dir.join(format!("{output_stem}_{:04}.jpg", idx + 1));
            if output.exists() {
                let _ = std::fs::remove_file(&output);
            }
            std::fs::rename(source, &output)
                .or_else(|_| {
                    std::fs::copy(source, &output).map(|_| ())?;
                    std::fs::remove_file(source)
                })
                .map_err(|e| ProcessingError {
                    file_path: output.to_string_lossy().to_string(),
                    message: format!("JPEG の無劣化抽出結果を移動できません: {e}"),
                })?;

            if let Some(tx) = progress_tx {
                let _ = tx.send(ProgressUpdate {
                    count: idx + 1,
                    total,
                    phase: ProgressPhase::Processing,
                });
            }
        }

        let _ = std::fs::remove_dir_all(&tmp_dir);
        Ok(Some(total))
    }

    fn list_pdf_images(
        pdfimages_path: &Path,
        input_pdf: &Path,
    ) -> Result<Vec<PdfImageInfo>, ProcessingError> {
        let output = Command::new(pdfimages_path)
            .arg("-list")
            .arg(input_pdf)
            .output()
            .map_err(|e| ProcessingError {
                file_path: input_pdf.to_string_lossy().to_string(),
                message: format!("pdfimages -list の実行に失敗しました: {e}"),
            })?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        Ok(parse_pdfimages_list(&String::from_utf8_lossy(
            &output.stdout,
        )))
    }

    fn can_losslessly_extract_jpegs(images: &[PdfImageInfo], total: usize) -> bool {
        if images.len() != total {
            return false;
        }

        let mut pages = vec![0usize; total + 1];
        for image in images {
            if image.page == 0 || image.page > total || !image.enc.eq_ignore_ascii_case("jpeg") {
                return false;
            }
            pages[image.page] += 1;
        }
        pages[1..].iter().all(|count| *count == 1)
    }

    /// 1 ページを画像ファイルに変換する。
    pub fn render_page(
        input_pdf: &Path,
        output_dir: &Path,
        output_stem: &str,
        page: usize,
        canvas_width: u32,
        format: OutputImageFormat,
    ) -> Result<PathBuf, ProcessingError> {
        let Some(pdftoppm_path) = Self::find_pdftoppm() else {
            return Err(ProcessingError {
                file_path: input_pdf.to_string_lossy().to_string(),
                message: "pdftoppm が見つかりません。Poppler をインストールするか tools/ に配置してください".to_string(),
            });
        };

        let prefix = output_dir.join(format!("{output_stem}_{page:04}"));
        let expected_output = prefix.with_extension(format.extension());

        let mut command = Command::new(pdftoppm_path);
        command
            .arg("-q")
            .arg("-f")
            .arg(page.to_string())
            .arg("-l")
            .arg(page.to_string())
            .arg("-singlefile")
            .arg(format.pdftoppm_flag());

        if format == OutputImageFormat::Jpeg {
            command
                .arg("-jpegopt")
                .arg(format!("quality={},optimize=y,progressive=n", PDF_QUALITY));
        }

        let output = command
            .arg("-scale-to-x")
            .arg(canvas_width.to_string())
            .arg("-scale-to-y")
            .arg("-1")
            .arg(input_pdf)
            .arg(&prefix)
            .output()
            .map_err(|e| ProcessingError {
                file_path: input_pdf.to_string_lossy().to_string(),
                message: format!("pdftoppm の実行に失敗しました: {e}"),
            })?;

        if !output.status.success() {
            return Err(ProcessingError {
                file_path: format!("{}#page={page}", input_pdf.to_string_lossy()),
                message: format!(
                    "pdftoppm が失敗しました: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            });
        }

        if !expected_output.is_file() {
            return Err(ProcessingError {
                file_path: format!("{}#page={page}", input_pdf.to_string_lossy()),
                message: format!(
                    "変換後ファイルが見つかりません: {}",
                    expected_output.to_string_lossy()
                ),
            });
        }

        Ok(expected_output)
    }
}

fn parse_pdfimages_list(stdout: &str) -> Vec<PdfImageInfo> {
    stdout
        .lines()
        .filter_map(|line| {
            let cols = line.split_whitespace().collect::<Vec<_>>();
            if cols.len() < 9 || cols.first() == Some(&"page") || cols.first() == Some(&"-") {
                return None;
            }
            let page = cols[0].parse::<usize>().ok()?;
            let enc = cols[8].to_string();
            Some(PdfImageInfo { page, enc })
        })
        .collect()
}

fn find_tool(tool_names: &[&str]) -> Option<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for tool_name in tool_names {
        for dir in [
            manifest_dir.join(PROJECT_TOOLS_DIR),
            manifest_dir.to_path_buf(),
        ] {
            let candidate = dir.join(tool_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            tool_names.iter().find_map(|tool_name| {
                let candidate = dir.join(tool_name);
                if candidate.is_file() {
                    Some(candidate)
                } else {
                    None
                }
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_processor::{ImageProcessor, JpegPage};
    use image::{ImageBuffer, Rgb, RgbImage};

    #[test]
    fn test_output_image_format_parse() {
        assert_eq!(
            OutputImageFormat::parse("jpg"),
            Some(OutputImageFormat::Jpeg)
        );
        assert_eq!(
            OutputImageFormat::parse("JPEG"),
            Some(OutputImageFormat::Jpeg)
        );
        assert_eq!(
            OutputImageFormat::parse("png"),
            Some(OutputImageFormat::Png)
        );
        assert_eq!(
            OutputImageFormat::parse("auto"),
            Some(OutputImageFormat::AutoLossless)
        );
        assert_eq!(OutputImageFormat::parse("webp"), None);
    }

    #[test]
    fn test_parse_pdfimages_list_and_extractable_heuristic() {
        let stdout = "page   num  type   width height color comp bpc  enc interp  object ID x-ppi y-ppi size ratio\n---- ----- ----- ------- ------ ----- ---- --- ----- ------ -------- -- ----- ----- ---- -----\n   1     0 image     120    170  rgb     3   8  jpeg   no         4  0    72    72 100B 0.1%\n   2     1 image     120    170  rgb     3   8  jpeg   no         8  0    72    72 100B 0.1%\n";
        let images = parse_pdfimages_list(stdout);
        assert_eq!(images.len(), 2);
        assert!(Pdf2ImgProcessor::can_losslessly_extract_jpegs(&images, 2));
    }

    #[test]
    fn test_page_count_and_render_smoke_when_poppler_exists() {
        if Pdf2ImgProcessor::find_pdfinfo().is_none() || Pdf2ImgProcessor::find_pdftoppm().is_none()
        {
            eprintln!("pdfinfo/pdftoppm が見つかりません。テストをスキップします。");
            return;
        }

        let tmp_dir = std::env::temp_dir().join(format!(
            "pdf2img_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let pdf_path = tmp_dir.join("sample.pdf");
        let img: RgbImage = ImageBuffer::from_pixel(120, 170, Rgb([240, 240, 240]));
        let data = ImageProcessor::encode_jpeg(&img).expect("encode failed");
        ImageProcessor::generate_pdf(
            vec![JpegPage {
                width: 120,
                height: 170,
                components: 3,
                data,
            }],
            &pdf_path.to_string_lossy(),
            120,
        )
        .expect("PDF generation failed");

        assert_eq!(Pdf2ImgProcessor::page_count(&pdf_path).unwrap(), 1);
        let output = Pdf2ImgProcessor::render_page(
            &pdf_path,
            &tmp_dir,
            "sample",
            1,
            100,
            OutputImageFormat::Jpeg,
        )
        .expect("render failed");
        assert!(output.is_file());
        assert_eq!(output.extension().and_then(|e| e.to_str()), Some("jpg"));

        let _ = std::fs::remove_dir_all(tmp_dir);
    }
}
