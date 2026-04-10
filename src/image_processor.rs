use crate::models::{ProcessingError, ProcessingResult, ProgressPhase, ProgressUpdate};
use crate::utils::constants::*;
use image::{ImageBuffer, Rgb, RgbImage};
use rayon::prelude::*;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// 画像処理と PDF 生成を担うコアモジュール
pub struct ImageProcessor;

impl ImageProcessor {
    /// A4 比率からキャンバス高さ（ピクセル）を計算する
    ///
    /// # Arguments
    /// * `width` - キャンバス幅（ピクセル）
    pub fn calculate_height(width: u32) -> u32 {
        ((width as f32 * A4_RATIO) + 0.5) as u32
    }

    /// 単一の JPEG 画像を読み込み、A4 キャンバスに中央配置して返す
    ///
    /// 処理フロー:
    /// 1. 画像読み込み
    /// 2. RGB 色空間に変換
    /// 3. アスペクト比保持でリサイズ
    /// 4. 白色キャンバスに中央配置
    pub fn process_single_image(
        file_path: &Path,
        canvas_w: u32,
        canvas_h: u32,
    ) -> Result<RgbImage, ProcessingError> {
        // 1. 画像読み込み
        let img = image::open(file_path).map_err(|e| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: format!("Failed to load image: {e}"),
        })?;

        // 2. RGB 色空間に変換
        let img_rgb = img.to_rgb8();

        // 3. アスペクト比保持でリサイズ
        let (orig_w, orig_h) = (img_rgb.width(), img_rgb.height());
        let scale = f32::min(
            canvas_w as f32 / orig_w as f32,
            canvas_h as f32 / orig_h as f32,
        );
        let new_w = ((orig_w as f32 * scale) + 0.5) as u32;
        let new_h = ((orig_h as f32 * scale) + 0.5) as u32;

        let img_resized = image::imageops::resize(
            &img_rgb,
            new_w,
            new_h,
            image::imageops::FilterType::Lanczos3,
        );

        // 4. 白色キャンバスに中央配置
        let mut canvas: RgbImage =
            ImageBuffer::from_pixel(canvas_w, canvas_h, Rgb(CANVAS_COLOR));

        let offset_x = ((canvas_w - new_w) / 2) as i64;
        let offset_y = ((canvas_h - new_h) / 2) as i64;
        image::imageops::overlay(&mut canvas, &img_resized, offset_x, offset_y);

        Ok(canvas)
    }

    /// 複数画像を並列処理して PDF を生成する（別スレッドで実行）
    ///
    /// # Arguments
    /// * `file_list`     - 処理するファイルパスの一覧
    /// * `canvas_width`  - キャンバス幅（ピクセル）
    /// * `output_path`   - 出力 PDF ファイルパス
    /// * `progress_tx`   - 進捗通知チャネル（省略可）
    /// * `finished_tx`   - 完了通知チャネル
    pub fn run(
        file_list: Vec<String>,
        canvas_width: u32,
        output_path: String,
        progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
        finished_tx: mpsc::Sender<ProcessingResult>,
    ) {
        std::thread::spawn(move || {
            let result = Self::run_thread(file_list, canvas_width, output_path, progress_tx);
            let _ = finished_tx.send(result);
        });
    }

    /// スレッド内のメイン処理（並列画像処理 → PDF 生成）
    fn run_thread(
        file_list: Vec<String>,
        canvas_width: u32,
        output_path: String,
        progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
    ) -> ProcessingResult {
        let canvas_height = Self::calculate_height(canvas_width);
        let total = file_list.len();

        // 進捗カウンター（スレッドセーフ）
        let counter = Arc::new(Mutex::new(0usize));

        // ファイルを並列処理（インデックスで順序を保持）
        let results: Vec<(usize, Result<RgbImage, ProcessingError>)> = file_list
            .par_iter()
            .enumerate()
            .map(|(idx, file_path)| {
                let result = Self::process_single_image(
                    Path::new(file_path),
                    canvas_width,
                    canvas_height,
                );

                // 進捗通知
                if let Some(ref tx) = progress_tx {
                    let mut count = counter.lock().unwrap();
                    *count += 1;
                    let _ = tx.send(ProgressUpdate {
                        count: *count,
                        total,
                        phase: ProgressPhase::Processing,
                    });
                }

                (idx, result)
            })
            .collect();

        // インデックス順にソート（入力ファイルの順序を保持）
        let mut ordered = results;
        ordered.sort_by_key(|(idx, _)| *idx);

        // 成功・失敗を分類
        let mut success_images: Vec<RgbImage> = Vec::new();
        let mut errors: Vec<ProcessingError> = Vec::new();

        for (_, result) in ordered {
            match result {
                Ok(img) => success_images.push(img),
                Err(e) => errors.push(e),
            }
        }

        if success_images.is_empty() {
            return ProcessingResult {
                success: false,
                success_count: 0,
                error_count: errors.len(),
                errors,
                output_path,
            };
        }

        // PDF 書き出しフェーズを通知
        if let Some(ref tx) = progress_tx {
            let _ = tx.send(ProgressUpdate {
                count: total,
                total,
                phase: ProgressPhase::Saving,
            });
        }

        // PDF 生成
        match Self::generate_pdf(&success_images, &output_path, canvas_width) {
            Ok(()) => ProcessingResult {
                success: true,
                success_count: success_images.len(),
                error_count: errors.len(),
                errors,
                output_path,
            },
            Err(msg) => {
                errors.push(ProcessingError {
                    file_path: "PDF Generation".to_string(),
                    message: msg,
                });
                ProcessingResult {
                    success: false,
                    success_count: 0,
                    error_count: errors.len(),
                    errors,
                    output_path,
                }
            }
        }
    }

    /// 処理済み画像群から PDF ファイルを生成する
    ///
    /// 各画像を A4 ページとして出力する。
    /// DPI は `canvas_width` から自動計算される。
    fn generate_pdf(
        images: &[RgbImage],
        output_path: &str,
        canvas_width: u32,
    ) -> Result<(), String> {
        use printpdf::*;
        use std::fs::File;
        use std::io::BufWriter;

        if images.is_empty() {
            return Err("No images to process".to_string());
        }

        // A4 サイズ（mm）
        let a4_width_mm = 210.0_f32;
        let a4_height_mm = 297.0_f32;

        // canvas_width が A4 幅に対応する DPI を計算
        // dpi = canvas_width / (a4_width_mm / 25.4)
        let dpi = canvas_width as f32 * 25.4 / a4_width_mm;

        let (doc, first_page, first_layer) = PdfDocument::new(
            APP_NAME,
            Mm(a4_width_mm),
            Mm(a4_height_mm),
            "Layer 1",
        );

        for (page_idx, img) in images.iter().enumerate() {
            let (current_page, current_layer) = if page_idx == 0 {
                (first_page, first_layer)
            } else {
                doc.add_page(Mm(a4_width_mm), Mm(a4_height_mm), "Layer 1")
            };

            let layer = doc.get_page(current_page).get_layer(current_layer);

            // RgbImage → image::DynamicImage → printpdf::Image
            let dyn_img = ::image::DynamicImage::ImageRgb8(img.clone());
            let pdf_image = printpdf::Image::from_dynamic_image(&dyn_img);

            // ページ全体に配置（translate_y=0 は PDF 座標系の底辺）
            pdf_image.add_to_layer(
                layer,
                ImageTransform {
                    translate_x: Some(Mm(0.0)),
                    translate_y: Some(Mm(0.0)),
                    rotate: None,
                    scale_x: None,
                    scale_y: None,
                    dpi: Some(dpi),
                },
            );
        }

        let file = File::create(output_path).map_err(|e| e.to_string())?;
        doc.save(&mut BufWriter::new(file))
            .map_err(|e| e.to_string())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_height() {
        // DEFAULT_WIDTH=1654 → height = round(1654 * 1.41421356 + 0.5) = 2339
        let h = ImageProcessor::calculate_height(1654);
        assert_eq!(h, 2339);

        // 幅0は0を返す
        assert_eq!(ImageProcessor::calculate_height(0), 0);

        // 小さい幅
        let h2 = ImageProcessor::calculate_height(100);
        assert_eq!(h2, ((100.0_f32 * A4_RATIO) + 0.5) as u32);
    }

    #[test]
    fn test_calculate_height_aspect_ratio() {
        // Tolerance accounts for integer rounding (±0.5px / height)
        for width in [500u32, 1000, 1654, 2000, 4000] {
            let height = ImageProcessor::calculate_height(width);
            let ratio = height as f32 / width as f32;
            assert!((ratio - A4_RATIO).abs() < 0.005, "ratio={ratio}");
        }
    }
}
