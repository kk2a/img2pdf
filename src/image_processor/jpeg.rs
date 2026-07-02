use super::{ImageProcessor, JpegPage};
use crate::models::ProcessingError;
use crate::utils::constants::*;
use image::RgbImage;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const PROJECT_TOOLS_DIR: &str = "tools";
const JPEGTRAN_TOOL_NAMES: &[&str] = if cfg!(windows) {
    &["jpegtran.exe", "jpegtran"]
} else {
    &["jpegtran", "jpegtran.exe"]
};

impl ImageProcessor {
    /// 利用可能な jpegtran を探す。
    ///
    /// 優先順は `tools/` 配下、プロジェクト直下、PATH 上の実行ファイル。
    pub fn find_jpegtran() -> Option<PathBuf> {
        static JPEGTRAN_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
        JPEGTRAN_PATH
            .get_or_init(|| {
                let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
                for tool_name in JPEGTRAN_TOOL_NAMES {
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
                        JPEGTRAN_TOOL_NAMES.iter().find_map(|tool_name| {
                            let candidate = dir.join(tool_name);
                            if candidate.is_file() {
                                Some(candidate)
                            } else {
                                None
                            }
                        })
                    })
                })
            })
            .clone()
    }

    /// JPEG を可逆最適化する（量子化は維持、ハフマン最適化のみ）
    ///
    /// - jpegtran がない、または失敗時は入力をそのまま返す
    /// - サイズが小さくならない場合も、入力をそのまま返す
    fn optimize_jpeg_lossless(jpeg_bytes: Vec<u8>) -> Vec<u8> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let Some(jpegtran_path) = Self::find_jpegtran() else {
            return jpeg_bytes;
        };

        let mut child = match Command::new(jpegtran_path)
            .args(["-copy", "none", "-optimize"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return jpeg_bytes,
        };

        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return jpeg_bytes;
        };

        let output = std::thread::scope(|scope| {
            let input = &jpeg_bytes;
            let writer = scope.spawn(move || {
                let mut stdin = stdin;
                stdin.write_all(input)
            });
            let output = child.wait_with_output();
            let write_ok = writer.join().map(|r| r.is_ok()).unwrap_or(false);
            if write_ok { output.ok() } else { None }
        });

        let output = match output {
            Some(output) if output.status.success() => output.stdout,
            _ => return jpeg_bytes,
        };

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

        // 白背景は PDF ページ側に任せ、画像本体だけ返す
        Self::resize_image(img_rgb, new_w, new_h).map_err(|msg| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: msg,
        })
    }

    /// zune-jpeg を使って JPEG をデコードする
    fn decode_image(file_path: &Path) -> Result<RgbImage, ProcessingError> {
        use zune_jpeg::JpegDecoder;
        use zune_jpeg::zune_core::bytestream::ZCursor;
        use zune_jpeg::zune_core::colorspace::ColorSpace;
        use zune_jpeg::zune_core::options::DecoderOptions;

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

        RgbImage::from_raw(new_w, new_h, dst.buffer().to_vec()).ok_or_else(|| {
            "fast_image_resize: pixel buffer size does not match dimensions".to_string()
        })
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

    /// JPEG を再圧縮せずに PDF 埋め込み用ページへ変換する。
    ///
    /// `--lossless` では画素リサイズも避けるため、キャンバス幅は表示倍率にのみ影響する。
    pub(super) fn process_jpeg_lossless(file_path: &Path) -> Result<JpegPage, ProcessingError> {
        let (width, height, components) = Self::jpeg_dimensions(file_path)?;
        let data = std::fs::read(file_path).map_err(|e| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: format!("Failed to read JPEG for lossless embedding: {e}"),
        })?;

        Ok(JpegPage {
            width,
            height,
            components,
            data,
        })
    }

    fn jpeg_dimensions(file_path: &Path) -> Result<(u32, u32, u8), ProcessingError> {
        use zune_jpeg::JpegDecoder;
        use zune_jpeg::zune_core::bytestream::ZCursor;

        let jpeg_bytes = std::fs::read(file_path).map_err(|e| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: format!("Failed to read file: {e}"),
        })?;
        let mut decoder = JpegDecoder::new(ZCursor::new(jpeg_bytes));
        decoder.decode_headers().map_err(|e| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: format!("zune-jpeg header decode failed: {e:?}"),
        })?;
        let info = decoder.info().ok_or_else(|| ProcessingError {
            file_path: file_path.to_string_lossy().to_string(),
            message: "zune-jpeg: failed to get image info after decoding headers".to_string(),
        })?;
        Ok((info.width as u32, info.height as u32, info.components))
    }

    /// 単一ファイルを読み込み → A4 キャンバスに配置 → JPEG エンコードまでを一括で行う
    ///
    /// `RgbImage` はエンコード後に即座にドロップし、メモリを解放する。
    pub(super) fn process_and_encode(
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
            components: 3,
            data: jpeg_data,
        })
    }
}
