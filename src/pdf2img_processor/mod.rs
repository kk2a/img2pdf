mod extraction;
mod render;
mod tools;
mod types;

pub use types::OutputImageFormat;

use crate::models::{ProcessingError, ProcessingResult, ProgressPhase, ProgressUpdate};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use types::PdfImageInfo;

#[cfg(test)]
use extraction::parse_pdfimages_list;

/// PDF から画像への変換を担うコアモジュール
pub struct Pdf2ImgProcessor;

impl Pdf2ImgProcessor {
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
