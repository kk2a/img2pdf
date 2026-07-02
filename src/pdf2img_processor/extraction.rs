use super::{Pdf2ImgProcessor, PdfImageInfo};
use crate::models::{ProcessingError, ProgressPhase, ProgressUpdate};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;

impl Pdf2ImgProcessor {
    pub(super) fn try_extract_lossless_jpegs(
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

    pub(super) fn list_pdf_images(
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

    pub(super) fn can_losslessly_extract_jpegs(images: &[PdfImageInfo], total: usize) -> bool {
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
}

pub(super) fn parse_pdfimages_list(stdout: &str) -> Vec<PdfImageInfo> {
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
