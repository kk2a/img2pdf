use super::config::{BookScanConfig, PageRange};
use super::image_ops::encode_pages;
use super::manifest::{BookScanManifest, BookScanStage, PageRecord};
use super::{blank, pdf, scantailor, superres};
use crate::pdf2img_processor::Pdf2ImgProcessor;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct BookScanReport {
    pub page_count: usize,
    pub blank_page_count: usize,
    pub output_pdf: PathBuf,
    pub work_dir: PathBuf,
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct BookScanProgress {
    pub stage: usize,
    pub total_stages: usize,
    pub message: String,
    pub completed: Option<usize>,
    pub total: Option<usize>,
}

pub struct BookScanProcessor;

impl BookScanProcessor {
    pub fn run(config: BookScanConfig) -> Result<BookScanReport, String> {
        Self::run_with_progress(config, |_| {})
    }

    pub fn run_with_progress<F>(
        config: BookScanConfig,
        progress: F,
    ) -> Result<BookScanReport, String>
    where
        F: Fn(BookScanProgress) + Sync,
    {
        config.validate()?;
        let started = Instant::now();
        let work_dir = config.resolved_work_dir();
        fs::create_dir_all(&work_dir)
            .map_err(|e| format!("本モード作業フォルダを作成できません: {e}"))?;
        let manifest_path = work_dir.join("manifest.json");

        if config.resume
            && let Ok(manifest) = BookScanManifest::load(&manifest_path)
            && configs_match_for_resume(&manifest.config, &config)
            && manifest.completed
            && config.output_pdf.is_file()
        {
            progress(BookScanProgress {
                stage: 5,
                total_stages: 5,
                message: "完了済みPDFを確認しました".to_string(),
                completed: Some(1),
                total: Some(1),
            });
            return Ok(BookScanReport {
                page_count: manifest.pages.len(),
                blank_page_count: manifest.pages.iter().filter(|page| page.is_blank).count(),
                output_pdf: config.output_pdf.clone(),
                work_dir,
                elapsed: started.elapsed(),
            });
        }

        let reusable = config
            .resume
            .then(|| BookScanManifest::load(&manifest_path).ok())
            .flatten()
            .filter(|manifest| configs_match_for_resume(&manifest.config, &config))
            .filter(|manifest| extracted_pages_valid(&manifest.pages));
        let mut manifest = if let Some(manifest) = reusable {
            progress_stage(&progress, 1, "検証済み入力画像を再利用します");
            manifest
        } else {
            progress_stage(&progress, 1, "入力ページを抽出して空白を判定します");
            let mut pages = extract_pages(&config, &work_dir)?;
            let blank_count = blank::classify_pages(&config, &mut pages)?;
            if config.blank_detection_enabled {
                progress_stage(
                    &progress,
                    1,
                    &format!("入力抽出完了（空白{blank_count}ページ）"),
                );
            }
            BookScanManifest::new(config.clone(), pages)
        };
        manifest.save_atomic(&manifest_path)?;

        if crop_pages_valid(&manifest.pages) {
            progress_stage(&progress, 2, "ScanTailor出力を再利用します");
        } else {
            progress_stage(&progress, 2, "ScanTailor前処理を実行します");
            scantailor::process(&config, &work_dir, &mut manifest.pages)?;
            manifest.save_atomic(&manifest_path)?;
        }

        if processed_pages_valid(&config, &manifest.pages) {
            progress_stage(&progress, 3, "超解像出力を再利用します");
        } else {
            if config.super_resolution == super::SuperResolutionMode::Off {
                progress_stage(&progress, 3, "超解像はOFFです");
            } else {
                progress_stage(
                    &progress,
                    3,
                    &format!(
                        "{:?} AI x{} → 出力x{}（GPU worker {}）",
                        config.super_resolution,
                        config.resolved_ai_scale(),
                        config.effective_output_scale(),
                        config.gpu_workers
                    ),
                );
            }
            let report_superres = |phase: &str, completed: usize, total: usize| {
                progress_units(&progress, 3, phase, completed, total);
            };
            superres::process(&config, &work_dir, &mut manifest.pages, &report_superres)?;
            manifest.save_atomic(&manifest_path)?;
        }

        if encoded_pages_valid(&config, &manifest.pages) {
            progress_stage(&progress, 4, "JPEGを再利用します");
        } else {
            progress_stage(&progress, 4, "階調・文字色・太さ調整とJPEG化を実行します");
            if !config.grayscale_pages.is_empty() {
                progress_stage(
                    &progress,
                    4,
                    &format!(
                        "指定ページ [{}] を1成分Gray化",
                        PageRange::format_list(&config.grayscale_pages)
                    ),
                );
            }
            let report_encoding = |phase: &str, completed: usize, total: usize| {
                progress_units(&progress, 4, phase, completed, total);
            };
            encode_pages(&config, &work_dir, &mut manifest.pages, &report_encoding)?;
            manifest.save_atomic(&manifest_path)?;
        }

        progress_stage(&progress, 5, "A4 PDFへ書き出します");
        pdf::write_pdf(
            &manifest.pages,
            &config.output_pdf,
            config.preserve_position,
        )?;
        for page in &mut manifest.pages {
            page.stage = BookScanStage::PdfWritten;
        }
        manifest.completed = true;
        manifest.save_atomic(&manifest_path)?;

        if !config.keep_work {
            cleanup_intermediates(&work_dir);
        }
        Ok(BookScanReport {
            page_count: manifest.pages.len(),
            blank_page_count: manifest.pages.iter().filter(|page| page.is_blank).count(),
            output_pdf: config.output_pdf.clone(),
            work_dir,
            elapsed: started.elapsed(),
        })
    }
}

fn progress_stage<F>(progress: &F, stage: usize, message: &str)
where
    F: Fn(BookScanProgress) + Sync,
{
    progress(BookScanProgress {
        stage,
        total_stages: 5,
        message: message.to_string(),
        completed: None,
        total: None,
    });
}

fn progress_units<F>(progress: &F, stage: usize, phase: &str, completed: usize, total: usize)
where
    F: Fn(BookScanProgress) + Sync,
{
    if total == 0 {
        return;
    }
    let percentage = completed.saturating_mul(100) / total;
    let message = format!("{phase}: {completed}/{total} ({percentage}%)");
    progress(BookScanProgress {
        stage,
        total_stages: 5,
        message,
        completed: Some(completed),
        total: Some(total),
    });
}

fn extract_pages(config: &BookScanConfig, work_dir: &Path) -> Result<Vec<PageRecord>, String> {
    let source_dir = work_dir.join("source");
    fs::create_dir_all(&source_dir).map_err(|e| format!("sourceフォルダを作成できません: {e}"))?;
    if config.input_path.is_dir() {
        extract_from_directory(config, &source_dir)
    } else if config
        .input_path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("pdf"))
    {
        extract_from_pdf(config, work_dir, &source_dir)
    } else {
        Err("入力にはPDFまたは画像フォルダを指定してください".to_string())
    }
}

fn extract_from_directory(
    config: &BookScanConfig,
    source_dir: &Path,
) -> Result<Vec<PageRecord>, String> {
    let mut files = fs::read_dir(&config.input_path)
        .map_err(|e| format!("入力フォルダを読めません: {e}"))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file() && is_supported_image(path))
        .collect::<Vec<_>>();
    files.sort_by_key(|path| natural_key(path));
    if files.is_empty() {
        return Err("入力フォルダにJPEG/PNG/TIFFがありません".to_string());
    }
    let range = config.pages.unwrap_or(PageRange {
        start: 1,
        end: files.len(),
    });
    if range.end > files.len() {
        return Err(format!(
            "ページ範囲{}-{}が画像数{}を超えています",
            range.start,
            range.end,
            files.len()
        ));
    }
    let selected = files[(range.start - 1)..range.end].to_vec();
    copy_as_pages(&selected, range.start, source_dir)
}

fn extract_from_pdf(
    config: &BookScanConfig,
    work_dir: &Path,
    source_dir: &Path,
) -> Result<Vec<PageRecord>, String> {
    let total = Pdf2ImgProcessor::page_count(&config.input_path).map_err(|error| error.message)?;
    let range = config.pages.unwrap_or(PageRange {
        start: 1,
        end: total,
    });
    if range.end > total {
        return Err(format!(
            "ページ範囲{}-{}がPDFページ数{}を超えています",
            range.start, range.end, total
        ));
    }
    let extract_tmp = work_dir.join("extract-tmp");
    if extract_tmp.exists() {
        fs::remove_dir_all(&extract_tmp)
            .map_err(|e| format!("旧extract一時フォルダを除去できません: {e}"))?;
    }
    fs::create_dir_all(&extract_tmp)
        .map_err(|e| format!("extract一時フォルダを作成できません: {e}"))?;

    let mut images = Vec::new();
    if let Some(pdfimages) = Pdf2ImgProcessor::find_pdfimages() {
        let prefix = extract_tmp.join("page");
        let output = Command::new(pdfimages)
            .arg("-f")
            .arg(range.start.to_string())
            .arg("-l")
            .arg(range.end.to_string())
            .arg("-j")
            .arg("-p")
            .arg(&config.input_path)
            .arg(&prefix)
            .output()
            .map_err(|e| format!("pdfimagesを実行できません: {e}"))?;
        if output.status.success() {
            images = list_images(&extract_tmp)
                .into_iter()
                .filter(|path| {
                    path.extension()
                        .and_then(|value| value.to_str())
                        .is_some_and(|value| {
                            value.eq_ignore_ascii_case("jpg") || value.eq_ignore_ascii_case("jpeg")
                        })
                })
                .collect();
        }
    }

    if images.len() != range.len() {
        for image in list_images(&extract_tmp) {
            let _ = fs::remove_file(image);
        }
        let pdftoppm = Pdf2ImgProcessor::find_pdftoppm()
            .ok_or_else(|| "PDFを直接抽出できず、pdftoppmも見つかりません".to_string())?;
        let prefix = extract_tmp.join("page");
        let output = Command::new(pdftoppm)
            .arg("-f")
            .arg(range.start.to_string())
            .arg("-l")
            .arg(range.end.to_string())
            .arg("-r")
            .arg(config.dpi.to_string())
            .arg("-jpeg")
            .arg(&config.input_path)
            .arg(&prefix)
            .output()
            .map_err(|e| format!("pdftoppmを実行できません: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "pdftoppmが失敗しました: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        images = list_images(&extract_tmp);
    }
    images.sort_by_key(|path| natural_key(path));
    if images.len() != range.len() {
        return Err(format!(
            "抽出ページ数が不一致です: expected={} actual={}",
            range.len(),
            images.len()
        ));
    }
    let records = copy_as_pages(&images, range.start, source_dir)?;
    let _ = fs::remove_dir_all(&extract_tmp);
    Ok(records)
}

fn copy_as_pages(
    inputs: &[PathBuf],
    first_page_number: usize,
    source_dir: &Path,
) -> Result<Vec<PageRecord>, String> {
    inputs
        .iter()
        .enumerate()
        .map(|(index, input)| {
            let page_number = first_page_number + index;
            let stem = format!("p{page_number:04}");
            let extension = input
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("png")
                .to_ascii_lowercase();
            let output = source_dir.join(format!("{stem}.{extension}"));
            fs::copy(input, &output)
                .map_err(|e| format!("{}をsourceへコピーできません: {e}", input.display()))?;
            let (width, height) = image::image_dimensions(&output)
                .map_err(|e| format!("{}の寸法を読めません: {e}", output.display()))?;
            Ok(PageRecord {
                index,
                page_number,
                stem,
                source_path: output,
                source_width: width,
                source_height: height,
                is_blank: false,
                blank_metrics: None,
                crop_path: None,
                crop_width: width,
                crop_height: height,
                restore_x: 0.0,
                restore_y: 0.0,
                processed_path: None,
                jpeg_path: None,
                stage: BookScanStage::Extracted,
                attempts: 0,
                error: None,
            })
        })
        .collect()
}

fn extracted_pages_valid(pages: &[PageRecord]) -> bool {
    !pages.is_empty()
        && pages.iter().all(|page| {
            page.source_path.is_file()
                && image::image_dimensions(&page.source_path)
                    .is_ok_and(|dimensions| dimensions == (page.source_width, page.source_height))
        })
}

fn crop_pages_valid(pages: &[PageRecord]) -> bool {
    extracted_pages_valid(pages)
        && pages.iter().all(|page| {
            page.stage >= BookScanStage::ScanTailored
                && page.crop_path.as_deref().is_some_and(|path| {
                    image::image_dimensions(path)
                        .is_ok_and(|dimensions| dimensions == (page.crop_width, page.crop_height))
                })
        })
}

fn processed_pages_valid(config: &BookScanConfig, pages: &[PageRecord]) -> bool {
    let scale = config.effective_output_scale();
    crop_pages_valid(pages)
        && pages.iter().all(|page| {
            page.stage >= BookScanStage::Upscaled
                && (page.is_blank && page.processed_path.is_none()
                    || !page.is_blank
                        && page.processed_path.as_deref().is_some_and(|path| {
                            image::image_dimensions(path).is_ok_and(|dimensions| {
                                dimensions == (page.crop_width * scale, page.crop_height * scale)
                            })
                        }))
        })
}

fn encoded_pages_valid(config: &BookScanConfig, pages: &[PageRecord]) -> bool {
    let scale = config.effective_output_scale();
    processed_pages_valid(config, pages)
        && pages.iter().all(|page| {
            page.stage >= BookScanStage::Encoded
                && page.jpeg_path.as_deref().is_some_and(|path| {
                    image::image_dimensions(path).is_ok_and(|dimensions| {
                        if page.is_blank {
                            dimensions == (page.source_width, page.source_height)
                        } else {
                            dimensions == (page.crop_width * scale, page.crop_height * scale)
                        }
                    })
                })
        })
}

fn list_images(directory: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file() && is_supported_image(path))
        .collect()
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "tif" | "tiff"
            )
        })
}

fn natural_key(path: &Path) -> (u64, String) {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let number = name
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    (number, name)
}

fn cleanup_intermediates(work_dir: &Path) {
    for directory in [
        "source",
        "scantailor-input",
        "scantailor",
        "preprocessed",
        "ai-input",
        "ai-output",
        "jpeg",
        "realesrgan-batches",
    ] {
        let path = work_dir.join(directory);
        if path.is_dir() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn configs_match_for_resume(saved: &BookScanConfig, requested: &BookScanConfig) -> bool {
    let mut saved = saved.clone();
    let mut requested = requested.clone();
    saved.resume = false;
    requested.resume = false;
    saved.keep_work = false;
    requested.keep_work = false;
    saved == requested
}
