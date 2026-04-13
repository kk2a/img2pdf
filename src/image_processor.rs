use crate::models::{ProcessingError, ProcessingResult, ProgressPhase, ProgressUpdate};
use crate::utils::constants::*;
use image::RgbImage;
use rayon::prelude::*;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

const PROJECT_TOOLS_DIR: &str = "tools";
#[cfg(windows)]
const JPEGTRAN_TOOL_NAME: &str = "jpegtran.exe";
#[cfg(not(windows))]
const JPEGTRAN_TOOL_NAME: &str = "jpegtran";

static MAX_PERFORMANCE_MODE: AtomicBool = AtomicBool::new(false);

/// CPU コア数の約 70% のスレッド数を算出する
///
/// rayon のグローバルプールに設定することで、長時間処理中もシステムへの
/// 負荷を抑え、CPU 稼働を 70% 程度に収める。
pub fn calc_worker_threads() -> usize {
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    if ImageProcessor::max_performance_mode() {
        return cpu_count;
    }
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

/// PDF に埋め込む 1 ページ分のデータ（JPEG エンコード済み）
///
/// `RgbImage` は encode 後すぐに解放されるため、長期保持しない。
pub struct JpegPage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// 画像処理と PDF 生成を担うコアモジュール
pub struct ImageProcessor;

impl ImageProcessor {
    /// 最大性能モードが有効か判定する
    ///
    /// CLI オプションから設定されたプロセス内フラグを参照する。
    pub fn max_performance_mode() -> bool {
        MAX_PERFORMANCE_MODE.load(Ordering::Relaxed)
    }

    /// 最大性能モードを設定する
    pub fn set_max_performance_mode(enabled: bool) {
        MAX_PERFORMANCE_MODE.store(enabled, Ordering::Relaxed);
    }

    /// A4 比率からキャンバス高さ（ピクセル）を計算する
    ///
    /// # Arguments
    /// * `width` - キャンバス幅（ピクセル）
    pub fn calculate_height(width: u32) -> u32 {
        ((width as f32 * A4_RATIO) + 0.5) as u32
    }

    /// JPEG を可逆最適化する（量子化は維持、ハフマン最適化のみ）
    ///
    /// - `tools/jpegtran(.exe)` がない、または失敗時は入力をそのまま返す
    /// - サイズが小さくならない場合も、入力をそのまま返す
    fn optimize_jpeg_lossless(jpeg_bytes: Vec<u8>) -> Vec<u8> {
        use std::process::Command;
        use std::process::Stdio;
        use std::time::{SystemTime, UNIX_EPOCH};

        let jpegtran_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(PROJECT_TOOLS_DIR)
            .join(JPEGTRAN_TOOL_NAME);
        if !jpegtran_path.exists() {
            return jpeg_bytes;
        }

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp_dir = std::env::temp_dir();
        let input_path = tmp_dir.join(format!("img2pdf_jpegtran_in_{unique}.jpg"));
        let output_path = tmp_dir.join(format!("img2pdf_jpegtran_out_{unique}.jpg"));

        if std::fs::write(&input_path, &jpeg_bytes).is_err() {
            return jpeg_bytes;
        }

        let status = match Command::new(&jpegtran_path)
            .args(["-copy", "none", "-optimize"])
            .arg(&input_path)
            .arg(&output_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(s) => s,
            Err(_e) => {
                let _ = std::fs::remove_file(&input_path);
                return jpeg_bytes;
            }
        };

        if !status.success() {
            let _ = std::fs::remove_file(&input_path);
            let _ = std::fs::remove_file(&output_path);
            return jpeg_bytes;
        }

        let output = match std::fs::read(&output_path) {
            Ok(bytes) => bytes,
            Err(_e) => {
                let _ = std::fs::remove_file(&input_path);
                let _ = std::fs::remove_file(&output_path);
                return jpeg_bytes;
            }
        };

        let _ = std::fs::remove_file(&input_path);
        let _ = std::fs::remove_file(&output_path);

        if output.len() < jpeg_bytes.len() {
            output
        } else {
            jpeg_bytes
        }
    }

    /// 単一の JPEG 画像を読み込み、A4 比率に収まるようリサイズして返す
    ///
    /// 処理フロー:
    /// 1. 画像デコード（zune-jpeg）
    /// 2. アスペクト比保持でリサイズ（fast_image_resize / Lanczos3）
    pub fn process_single_image(
        file_path: &Path,
        canvas_w: u32,
        canvas_h: u32,
    ) -> Result<RgbImage, ProcessingError> {
        // 1. 画像デコード
        let img_rgb = Self::decode_image(file_path)?;

        // 2. アスペクト比保持でリサイズ
        let (orig_w, orig_h) = (img_rgb.width(), img_rgb.height());
        let scale = f32::min(
            canvas_w as f32 / orig_w as f32,
            canvas_h as f32 / orig_h as f32,
        );
        let new_w = ((orig_w as f32 * scale) + 0.5) as u32;
        let new_h = ((orig_h as f32 * scale) + 0.5) as u32;

        // 3. 白背景は PDF ページ側に任せ、画像本体だけ返す
        Self::resize_image(img_rgb, new_w, new_h).map_err(|msg| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: msg,
        })
    }

    /// zune-jpeg を使って JPEG をデコードする
    fn decode_image(file_path: &Path) -> Result<RgbImage, ProcessingError> {
        use zune_jpeg::zune_core::bytestream::ZCursor;
        use zune_jpeg::zune_core::colorspace::ColorSpace;
        use zune_jpeg::zune_core::options::DecoderOptions;
        use zune_jpeg::JpegDecoder;

        let jpeg_bytes = std::fs::read(file_path).map_err(|e| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: format!("Failed to read file: {e}"),
        })?;

        let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
        let mut decoder = JpegDecoder::new_with_options(ZCursor::new(jpeg_bytes), options);

        decoder.decode_headers().map_err(|e| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: format!("zune-jpeg header decode failed: {e:?}"),
        })?;

        let info = decoder.info().ok_or_else(|| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: "zune-jpeg: failed to get image info after decoding headers".to_string(),
        })?;
        let (width, height) = (info.width as u32, info.height as u32);

        let pixels = decoder.decode().map_err(|e| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: format!("zune-jpeg decode failed: {e:?}"),
        })?;

        RgbImage::from_raw(width, height, pixels).ok_or_else(|| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: "zune-jpeg: pixel buffer size does not match image dimensions".to_string(),
        })
    }

    /// fast_image_resize クレートの Lanczos3 フィルタでリサイズする
    fn resize_image(img: RgbImage, new_w: u32, new_h: u32) -> Result<RgbImage, String> {
        use fast_image_resize::images::Image;
        use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

        let (orig_w, orig_h) = (img.width(), img.height());
        let raw = img.into_raw();
        let src = Image::from_vec_u8(orig_w, orig_h, raw, PixelType::U8x3)
            .map_err(|e| format!("fast_image_resize: failed to create source image: {e}"))?;
        let mut dst = Image::new(new_w, new_h, PixelType::U8x3);

        let mut resizer = Resizer::new();
        resizer
            .resize(
                &src,
                &mut dst,
                &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)),
            )
            .map_err(|e| format!("fast_image_resize: resize failed: {e}"))?;

        RgbImage::from_raw(new_w, new_h, dst.buffer().to_vec())
            .ok_or_else(|| "fast_image_resize: pixel buffer size does not match dimensions".to_string())
    }

    /// `RgbImage` を JPEG バイト列（quality=`PDF_QUALITY`）にエンコードする（image クレート使用）
    pub fn encode_jpeg(img: &RgbImage) -> Result<Vec<u8>, String> {
        use image::codecs::jpeg::JpegEncoder;
        let mut jpeg_bytes: Vec<u8> = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg_bytes, PDF_QUALITY)
            .encode_image(img)
            .map_err(|e| e.to_string())?;
        Ok(jpeg_bytes)
    }

    /// 単一ファイルを読み込み → A4 キャンバスに配置 → JPEG エンコードまでを一括で行う
    ///
    /// `RgbImage` はエンコード後に即座にドロップし、メモリを解放する。
    fn process_and_encode(
        file_path: &Path,
        canvas_w: u32,
        canvas_h: u32,
    ) -> Result<JpegPage, ProcessingError> {
        let canvas = Self::process_single_image(file_path, canvas_w, canvas_h)?;
        let (w, h) = (canvas.width(), canvas.height());
        // JPEG エンコード後に canvas (RgbImage) をドロップ → メモリ解放
        let jpeg_data = Self::encode_jpeg(&canvas).map_err(|msg| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: format!("JPEG encode failed: {msg}"),
        })?;
        let jpeg_data = Self::optimize_jpeg_lossless(jpeg_data);
        Ok(JpegPage {
            width: w,
            height: h,
            data: jpeg_data,
        })
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

    /// スレッド内のメイン処理（並列画像処理＋JPEG エンコード → PDF 生成）
    fn run_thread(
        file_list: Vec<String>,
        canvas_width: u32,
        output_path: String,
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
                let result = Self::process_and_encode(
                    Path::new(file_path),
                    canvas_width,
                    canvas_height,
                );

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

    /// JPEG エンコード済みページ群から PDF ファイルを生成する
    ///
    /// JPEG バイトはすでに並列処理フェーズで生成済みであり、
    /// このフェーズでは再エンコードを行わない。
    /// pdf-writer を使用して直接 PDF バイト列を構築し書き出す。
    pub fn generate_pdf(
        pages: Vec<JpegPage>,
        output_path: &str,
        _canvas_width: u32,
    ) -> Result<(), String> {
        use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};
        use std::fs;

        if pages.is_empty() {
            return Err("No images to process".to_string());
        }

        // A4 サイズ（pt）: 1pt = 1/72 inch
        const A4_W: f32 = 595.28;
        const A4_H: f32 = 841.89;

        let mut pdf = Pdf::new();
        let total = pages.len();

        // オブジェクト ID の割り当て:
        //   1: catalog
        //   2: page tree
        //   3..3+total-1: page
        //   3+total..3+2*total-1: image XObject
        //   3+2*total..3+3*total-1: content stream
        let catalog_id = Ref::new(1);
        let page_tree_id = Ref::new(2);
        let base = 3_i32;
        let page_ids: Vec<Ref> = (0..total)
            .map(|i| Ref::new(base + i as i32))
            .collect();
        let image_ids: Vec<Ref> = (0..total)
            .map(|i| Ref::new(base + total as i32 + i as i32))
            .collect();
        let content_ids: Vec<Ref> = (0..total)
            .map(|i| Ref::new(base + 2 * total as i32 + i as i32))
            .collect();

        // catalog → page tree
        pdf.catalog(catalog_id).pages(page_tree_id);

        // page tree
        pdf.pages(page_tree_id)
            .kids(page_ids.iter().copied())
            .count(total as i32);

        // 各ページを書き出す
        let image_name = Name(b"Im0");
        for (i, page) in pages.into_iter().enumerate() {
            // ページ定義
            let mut pdf_page = pdf.page(page_ids[i]);
            pdf_page.media_box(Rect::new(0.0, 0.0, A4_W, A4_H));
            pdf_page.parent(page_tree_id);
            pdf_page.contents(content_ids[i]);
            pdf_page
                .resources()
                .x_objects()
                .pair(image_name, image_ids[i]);
            pdf_page.finish();

            // 画像 XObject（JPEG バイトをそのまま DCTDecode で埋め込む）
            let mut img = pdf.image_xobject(image_ids[i], &page.data);
            img.filter(Filter::DctDecode);
            img.width(page.width as i32);
            img.height(page.height as i32);
            img.color_space().device_rgb();
            img.bits_per_component(8);
            img.finish();

            // コンテンツストリーム: 画像をアスペクト比維持で中央配置
            // PDF 座標系は左下原点。XObject は 1×1 なので行列で拡大・平行移動する。
            let sx = A4_W / page.width as f32;
            let sy = A4_H / page.height as f32;
            let scale = sx.min(sy);
            let draw_w = page.width as f32 * scale;
            let draw_h = page.height as f32 * scale;
            let offset_x = (A4_W - draw_w) * 0.5;
            let offset_y = (A4_H - draw_h) * 0.5;

            let mut content = Content::new();
            content.save_state();
            content.transform([draw_w, 0.0, 0.0, draw_h, offset_x, offset_y]);
            content.x_object(image_name);
            content.restore_state();
            pdf.stream(content_ids[i], &content.finish());
        }

        let bytes = pdf.finish();
        fs::write(output_path, &bytes).map_err(|e| e.to_string())?;

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

    /// テスト用 JpegPage を生成する
    fn make_jpeg_page(width: u32, height: u32, color: [u8; 3]) -> JpegPage {
        let img = make_test_image(width, height, color);
        let data = ImageProcessor::encode_jpeg(&img).expect("encode failed");
        JpegPage { width, height, data }
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
        let (page_w, page_h) = (200u32, 283u32);
        let page = make_jpeg_page(page_w, page_h, [240, 240, 240]);
        ImageProcessor::generate_pdf(vec![page], &path_str, page_w)
            .expect("PDF generation failed");

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
        ImageProcessor::generate_pdf(pages, &path_str, 200)
            .expect("PDF generation failed");

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
