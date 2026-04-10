/// 画像プロセッサ単体テスト
///
/// RUST_MIGRATION_GUIDE.md §13 テスト項目チェックリストに対応
#[cfg(test)]
mod tests {
    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageBuffer, Rgb, RgbImage};
    use img2pdf::image_processor::{calc_worker_threads, ImageProcessor};
    use img2pdf::utils::constants::A4_RATIO;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    // ─── calculate_height ───────────────────────────────────────────────

    #[test]
    fn test_calculate_height_default_width() {
        // DEFAULT_WIDTH = 1654 → height = round(1654 * 1.41421356) = 2339
        let h = ImageProcessor::calculate_height(1654);
        assert_eq!(h, 2339);
    }

    #[test]
    fn test_calculate_height_zero() {
        assert_eq!(ImageProcessor::calculate_height(0), 0);
    }

    #[test]
    fn test_calculate_height_aspect_ratio() {
        // For larger widths, the ratio is very close to A4_RATIO.
        // Small widths have more rounding error (±0.5px / height), so we use
        // a conservative tolerance of 0.5% which covers all practical inputs.
        for width in [500u32, 1000, 1654, 2000, 4000] {
            let height = ImageProcessor::calculate_height(width);
            let ratio = height as f32 / width as f32;
            assert!(
                (ratio - A4_RATIO).abs() < 0.005,
                "width={width} ratio={ratio:.4} expected≈{A4_RATIO:.4}"
            );
        }
    }

    // ─── スレッド数制限 ──────────────────────────────────────────────────

    #[test]
    fn test_worker_threads_at_most_cpu_count() {
        let n = calc_worker_threads();
        let cpu_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        assert!(n >= 1, "Thread count must be at least 1");
        assert!(n <= cpu_count, "Thread count ({n}) must not exceed CPU count ({cpu_count})");
    }

    // ─── CLI 引数解析 ────────────────────────────────────────────────────

    #[test]
    fn test_cli_parse_no_args_returns_none() {
        use img2pdf::cli::parse_args;
        assert!(parse_args(&[]).is_none());
    }

    #[test]
    fn test_cli_parse_one_arg_returns_none() {
        use img2pdf::cli::parse_args;
        let args = vec!["folder".to_string()];
        assert!(parse_args(&args).is_none());
    }

    #[test]
    fn test_cli_parse_two_args_returns_some() {
        use img2pdf::cli::parse_args;
        use img2pdf::utils::constants::DEFAULT_WIDTH;
        let args = vec!["/some/folder".to_string(), "/out.pdf".to_string()];
        let parsed = parse_args(&args).expect("Should parse with 2 args");
        assert_eq!(parsed.input_folder, "/some/folder");
        assert_eq!(parsed.output_path, "/out.pdf");
        assert_eq!(parsed.canvas_width, DEFAULT_WIDTH);
    }

    #[test]
    fn test_cli_parse_custom_width() {
        use img2pdf::cli::parse_args;
        let args = vec![
            "/folder".to_string(),
            "/out.pdf".to_string(),
            "--width".to_string(),
            "800".to_string(),
        ];
        let parsed = parse_args(&args).expect("Should parse with --width");
        assert_eq!(parsed.canvas_width, 800);
    }

    // ─── jpegtran 最小疎通 ─────────────────────────────────────────────

    fn find_jpegtran() -> Option<PathBuf> {
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let candidates = [
            base.join("tools").join("jpegtran.exe"),
            base.join("tools").join("jpegtran"),
            base.join("jpegtran.exe"),
            base.join("jpegtran"),
        ];
        candidates.into_iter().find(|p| p.exists())
    }

    #[test]
    fn test_jpegtran_lossless_minimal_smoke() {
        let jpegtran = find_jpegtran().expect(
            "jpegtran が見つかりません。tools/jpegtran.exe か プロジェクトルート/jpegtran.exe を配置してください。",
        );

        // 単色の小さい JPEG を作成
        let img: RgbImage = ImageBuffer::from_pixel(32, 32, Rgb([220, 220, 220]));
        let mut input_jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut input_jpeg, 80)
            .encode_image(&img)
            .expect("failed to create input jpeg");

        // jpegtran はこの環境では input/output ファイル指定が必要
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = std::env::temp_dir();
        let in_path = tmp.join(format!("img2pdf_smoke_in_{unique}.jpg"));
        let out_path = tmp.join(format!("img2pdf_smoke_out_{unique}.jpg"));
        std::fs::write(&in_path, &input_jpeg).expect("failed to write input jpeg");

        let out = Command::new(jpegtran)
            .args(["-copy", "none", "-optimize"])
            .arg(&in_path)
            .arg(&out_path)
            .output()
            .expect("failed to run jpegtran");

        assert!(
            out.status.success(),
            "jpegtran failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out_bytes = std::fs::read(&out_path).expect("failed to read jpegtran output file");
        assert!(!out_bytes.is_empty(), "jpegtran output is empty");
        assert!(
            out_bytes.starts_with(&[0xFF, 0xD8]),
            "jpegtran output is not JPEG"
        );

        let _ = std::fs::remove_file(&in_path);
        let _ = std::fs::remove_file(&out_path);
    }
}
