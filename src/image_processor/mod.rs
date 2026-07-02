mod jpeg;
mod pdf;
mod threading;
mod types;

pub use threading::{calc_worker_threads, init_thread_pool};
pub use types::JpegPage;

use crate::models::{ProcessingError, ProcessingResult, ProgressPhase, ProgressUpdate};
use crate::utils::constants::*;
use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

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
        lossless: bool,
        progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
        finished_tx: mpsc::Sender<ProcessingResult>,
    ) {
        std::thread::spawn(move || {
            let result =
                Self::run_thread(file_list, canvas_width, output_path, lossless, progress_tx);
            let _ = finished_tx.send(result);
        });
    }

    /// スレッド内のメイン処理（並列画像処理＋JPEG エンコード → PDF 生成）
    fn run_thread(
        file_list: Vec<String>,
        canvas_width: u32,
        output_path: String,
        lossless: bool,
        progress_tx: Option<mpsc::Sender<ProgressUpdate>>,
    ) -> ProcessingResult {
        let canvas_height = Self::calculate_height(canvas_width);
        let total = file_list.len();

        // 進捗カウンター: Mutex を使わず AtomicUsize でロックフリーに管理
        let counter = Arc::new(AtomicUsize::new(0));

        // 並列処理: ロード・リサイズ・JPEG エンコードを一括で実施し
        // RgbImage をすぐに解放してメモリ圧迫を抑える
        let mut results: Vec<(usize, Result<JpegPage, ProcessingError>)> = file_list
            .par_iter()
            .enumerate()
            .map(|(idx, file_path)| {
                let path = Path::new(file_path);
                let result = if lossless {
                    Self::process_jpeg_lossless(path)
                } else {
                    Self::process_and_encode(path, canvas_width, canvas_height)
                };

                // ロックフリーで進捗カウンタをインクリメント
                if let Some(ref tx) = progress_tx {
                    let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                    let _ = tx.send(ProgressUpdate {
                        count,
                        total,
                        phase: ProgressPhase::Processing,
                    });
                }

                (idx, result)
            })
            .collect();

        // インデックス順にソート（入力ファイルの順序を保持）
        results.sort_unstable_by_key(|(idx, _)| *idx);

        // 成功・失敗を分類（中間 Vec を減らし、直接 partition）
        let mut errors: Vec<ProcessingError> = Vec::new();
        let success_pages: Vec<JpegPage> = results
            .into_iter()
            .filter_map(|(_, result)| match result {
                Ok(page) => Some(page),
                Err(e) => {
                    errors.push(e);
                    None
                }
            })
            .collect();

        if success_pages.is_empty() {
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

        let success_count = success_pages.len();
        // PDF 生成（JPEG バイトはすでに並列フェーズで揃っている）
        match Self::generate_pdf(success_pages, &output_path, canvas_width) {
            Ok(()) => ProcessingResult {
                success: true,
                success_count,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb, RgbImage};
    use std::sync::Arc;

    /// テスト用の単色 RGB 画像を生成する
    fn make_test_image(width: u32, height: u32, color: [u8; 3]) -> RgbImage {
        ImageBuffer::from_pixel(width, height, Rgb(color))
    }

    /// テスト用 JpegPage を生成する
    fn make_jpeg_page(width: u32, height: u32, color: [u8; 3]) -> JpegPage {
        let img = make_test_image(width, height, color);
        let data = ImageProcessor::encode_jpeg(&img).expect("encode failed");
        JpegPage {
            width,
            height,
            components: 3,
            data,
        }
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
            Rgb([
                (x * 3 % 256) as u8,
                (y * 5 % 256) as u8,
                ((x + y) % 256) as u8,
            ])
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
        let (page_w, page_h) = (200u32, 283u32);
        let page = make_jpeg_page(page_w, page_h, [240, 240, 240]);
        ImageProcessor::generate_pdf(vec![page], &path_str, page_w).expect("PDF generation failed");

        let pdf_size = std::fs::metadata(&tmp).unwrap().len();
        let raw_rgb_size = (page_w as u64 * page_h as u64 * 3) as u64;

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

    /// generate_pdf が複数ページを正しく処理することを確認
    #[test]
    fn test_generate_pdf_multiple_pages() {
        let tmp = std::env::temp_dir().join("img2pdf_test_multi.pdf");
        let path_str = tmp.to_string_lossy().to_string();

        let pages = vec![
            make_jpeg_page(200, 283, [255, 0, 0]),
            make_jpeg_page(200, 283, [0, 255, 0]),
            make_jpeg_page(200, 283, [0, 0, 255]),
        ];
        ImageProcessor::generate_pdf(pages, &path_str, 200).expect("PDF generation failed");

        let content = std::fs::read(&tmp).unwrap();
        assert!(content.starts_with(b"%PDF"), "Output should be valid PDF");

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

    /// AtomicUsize ベースの進捗カウンタが並列アクセスで正確に動作することを確認
    #[test]
    fn test_atomic_counter_correctness() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let counter = Arc::new(AtomicUsize::new(0));
        let n = 100usize;

        (0..n).into_par_iter().for_each(|_| {
            counter.fetch_add(1, Ordering::Relaxed);
        });

        assert_eq!(
            counter.load(Ordering::SeqCst),
            n,
            "Atomic counter should equal {n} after {n} increments"
        );
    }

    #[test]
    fn test_max_performance_mode_flag() {
        ImageProcessor::set_max_performance_mode(false);
        assert!(!ImageProcessor::max_performance_mode());

        ImageProcessor::set_max_performance_mode(true);
        assert!(ImageProcessor::max_performance_mode());

        ImageProcessor::set_max_performance_mode(false);
        assert!(!ImageProcessor::max_performance_mode());
    }
}
