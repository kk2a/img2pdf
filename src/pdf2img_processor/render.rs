use super::{OutputImageFormat, Pdf2ImgProcessor};
use crate::models::ProcessingError;
use crate::utils::constants::PDF_QUALITY;
use std::path::{Path, PathBuf};
use std::process::Command;

impl Pdf2ImgProcessor {
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
