use crate::models::{ProcessingError, ProcessingResult, ProgressPhase, ProgressUpdate};
use crate::utils::constants::*;
use image::{ImageBuffer, Rgb, RgbImage};
use rayon::prelude::*;
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// CPU コア数の約 70% のスレッド数を算出する
///
/// rayon のグローバルプールに設定することで、長時間処理中もシステムへの
/// 負荷を抑え、CPU 稼働を 70% 程度に収める。
pub fn calc_worker_threads() -> usize {
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    // 70% に丸める（最低 1 スレッド）
    std::cmp::max(1, (cpu_count as f64 * 0.7).round() as usize)
}

/// rayon グローバルスレッドプールを CPU 数の 70% に初期化する
///
/// アプリ起動直後に一度呼べばよい。既に初期化済みの場合は無視される。
pub fn init_thread_pool() {
    let num_threads = calc_worker_threads();
    // エラー（二重初期化など）は無視して続行
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build_global();
}

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

    /// `RgbImage` を JPEG バイト列（quality=`PDF_QUALITY`）にエンコードする
    pub fn encode_jpeg(img: &RgbImage) -> Result<Vec<u8>, String> {
        use image::codecs::jpeg::JpegEncoder;
        let mut jpeg_bytes: Vec<u8> = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg_bytes, PDF_QUALITY)
            .encode_image(img)
            .map_err(|e| e.to_string())?;
        Ok(jpeg_bytes)
    }

    /// 処理済み画像群から PDF ファイルを生成する
    ///
    /// 各画像を JPEG (DCTDecode) で圧縮してから PDF ページとして埋め込む。
    /// DPI は `canvas_width` から自動計算される。
    pub fn generate_pdf(
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

            // JPEG バイト列にエンコード（DCTDecode フィルタで PDF サイズを削減）
            let jpeg_bytes = Self::encode_jpeg(img)?;

            // ImageXObject を DCTDecode フィルタ付きで構築
            let pdf_image = Image {
                image: ImageXObject {
                    width: Px(img.width() as usize),
                    height: Px(img.height() as usize),
                    color_space: ColorSpace::Rgb,
                    bits_per_component: ColorBits::Bit8,
                    interpolate: true,
                    image_data: jpeg_bytes,
                    image_filter: Some(ImageFilter::DCT),
                    smask: None,
                    clipping_bbox: None,
                },
            };

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
    use image::{ImageBuffer, Rgb};

    /// テスト用の単色 RGB 画像を生成する
    fn make_test_image(width: u32, height: u32, color: [u8; 3]) -> RgbImage {
        ImageBuffer::from_pixel(width, height, Rgb(color))
    }

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

    /// JPEG エンコードが正常に動作し、生の RGB より大幅に小さいことを確認
    #[test]
    fn test_encode_jpeg_compresses_significantly() {
        let img = make_test_image(400, 300, [200, 100, 50]);
        let jpeg = ImageProcessor::encode_jpeg(&img).expect("JPEG encode failed");

        // JPEG バイト列は有効な JPEG ヘッダ (FFD8) を持つ
        assert!(jpeg.starts_with(&[0xFF, 0xD8]), "Not a valid JPEG header");

        // 圧縮率: 生 RGB = 400*300*3 = 360000 bytes, JPEG は大幅に小さいはず
        let raw_size = (img.width() * img.height() * 3) as usize;
        assert!(
            jpeg.len() < raw_size / 5,
            "JPEG ({} bytes) should be at least 5x smaller than raw ({} bytes)",
            jpeg.len(),
            raw_size
        );
    }

    /// JPEG 品質が PDF_QUALITY 定数通りに適用されていることを確認
    /// （低品質と高品質でファイルサイズが異なる）
    #[test]
    fn test_jpeg_quality_affects_size() {
        use image::codecs::jpeg::JpegEncoder;

        // ランダムっぽいパターンの画像（圧縮率の差が出やすい）
        let img: RgbImage = ImageBuffer::from_fn(200, 200, |x, y| {
            Rgb([(x * 3 % 256) as u8, (y * 5 % 256) as u8, ((x + y) % 256) as u8])
        });

        let mut low_q: Vec<u8> = Vec::new();
        JpegEncoder::new_with_quality(&mut low_q, 10)
            .encode_image(&img)
            .unwrap();

        let mut high_q: Vec<u8> = Vec::new();
        JpegEncoder::new_with_quality(&mut high_q, 95)
            .encode_image(&img)
            .unwrap();

        assert!(
            low_q.len() < high_q.len(),
            "Low quality ({} bytes) should be smaller than high quality ({} bytes)",
            low_q.len(),
            high_q.len()
        );

        // PDF_QUALITY (80) は中品質なので高品質より小さいはず
        let our_q = ImageProcessor::encode_jpeg(&img).unwrap();
        assert!(
            our_q.len() < high_q.len(),
            "PDF_QUALITY=80 ({} bytes) should be smaller than quality=95 ({} bytes)",
            our_q.len(),
            high_q.len()
        );
    }

    /// PDF 生成が成功し、ファイルサイズが合理的な範囲に収まることを確認
    #[test]
    fn test_generate_pdf_size_is_reasonable() {
        let tmp = std::env::temp_dir().join("img2pdf_test_size.pdf");
        let path_str = tmp.to_string_lossy().to_string();

        // 小さめのテスト画像 (200x283 ≈ A4比率)
        let img = make_test_image(200, 283, [240, 240, 240]);
        ImageProcessor::generate_pdf(&[img.clone()], &path_str, 200)
            .expect("PDF generation failed");

        let pdf_size = std::fs::metadata(&tmp).unwrap().len();
        let raw_rgb_size = (img.width() * img.height() * 3) as u64;

        // PDF は生 RGB より大幅に小さい（JPEG 圧縮が効いている）
        assert!(
            pdf_size < raw_rgb_size,
            "PDF ({pdf_size} bytes) should be smaller than raw RGB ({raw_rgb_size} bytes)"
        );

        // PDFヘッダ確認
        let content = std::fs::read(&tmp).unwrap();
        assert!(
            content.starts_with(b"%PDF"),
            "Output should start with %PDF header"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    /// calc_worker_threads が常に 1 以上を返すことを確認
    #[test]
    fn test_calc_worker_threads() {
        let n = calc_worker_threads();
        assert!(n >= 1, "Worker threads must be at least 1, got {n}");

        // 利用可能な CPU 数を超えないことも確認
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        assert!(
            n <= cpu_count,
            "Worker threads ({n}) must not exceed CPU count ({cpu_count})"
        );
    }
}
