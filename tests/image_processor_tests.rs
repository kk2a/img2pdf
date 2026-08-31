/// 画像プロセッサ単体テスト
///
/// RUST_MIGRATION_GUIDE.md §13 テスト項目チェックリストに対応
#[cfg(test)]
mod tests {
    use image::codecs::jpeg::JpegEncoder;
    use image::{ImageBuffer, Rgb, RgbImage};
    use img2pdf::image_processor::{ImageProcessor, calc_worker_threads};
    use img2pdf::utils::constants::A4_RATIO;
    use std::io::Write;
    use std::process::{Command, Stdio};

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
        assert!(
            n <= cpu_count,
            "Thread count ({n}) must not exceed CPU count ({cpu_count})"
        );
    }

    // ─── CLI 引数解析 ────────────────────────────────────────────────────

    #[test]
    fn test_cli_parse_no_args_returns_none() {
        use img2pdf::cli::parse_args;
        assert!(parse_args(&[]).is_none());
    }

    #[test]
    fn test_cli_parse_one_arg_uses_default_output() {
        use img2pdf::cli::parse_args;
        let args = vec!["folder".to_string()];
        let parsed = parse_args(&args).expect("one input arg should use default output path");
        assert_eq!(parsed.input_folder, "folder");
        assert!(parsed.output_path.ends_with(".pdf"));
    }

    #[test]
    fn test_cli_parse_two_args_returns_some() {
        use img2pdf::cli::{CliMode, parse_args};
        use img2pdf::utils::constants::DEFAULT_WIDTH;
        let args = vec!["/some/folder".to_string(), "/out.pdf".to_string()];
        let parsed = parse_args(&args).expect("Should parse with 2 args");
        assert_eq!(parsed.mode, CliMode::Img2Pdf);
        assert_eq!(parsed.input_folder, "/some/folder");
        assert_eq!(parsed.output_path, "/out.pdf");
        assert_eq!(parsed.canvas_width, DEFAULT_WIDTH);
    }

    #[test]
    fn test_cli_parse_pdf2img_subcommand() {
        use img2pdf::cli::{CliMode, parse_args};
        use img2pdf::pdf2img_processor::OutputImageFormat;
        let args = vec![
            "pdf2img".to_string(),
            "/in.pdf".to_string(),
            "/out".to_string(),
            "--format".to_string(),
            "png".to_string(),
            "--width".to_string(),
            "800".to_string(),
        ];
        let parsed = parse_args(&args).expect("Should parse pdf2img args");
        assert_eq!(parsed.mode, CliMode::Pdf2Img);
        assert_eq!(parsed.input_folder, "/in.pdf");
        assert_eq!(parsed.output_path, "/out");
        assert_eq!(parsed.canvas_width, 800);
        assert_eq!(parsed.output_format, OutputImageFormat::Png);
    }

    #[test]
    fn test_cli_parse_pdf2img_default_auto_lossless() {
        use img2pdf::cli::parse_args;
        use img2pdf::pdf2img_processor::OutputImageFormat;
        let args = vec!["pdf2img".to_string(), "/in.pdf".to_string()];
        let parsed = parse_args(&args).expect("Should parse pdf2img args");
        assert_eq!(parsed.output_format, OutputImageFormat::AutoLossless);
    }

    #[test]
    fn test_cli_parse_img2pdf_lossless() {
        use img2pdf::cli::{CliMode, parse_args};
        let args = vec![
            "/folder".to_string(),
            "/out.pdf".to_string(),
            "--lossless".to_string(),
        ];
        let parsed = parse_args(&args).expect("Should parse with --lossless");
        assert_eq!(parsed.mode, CliMode::Img2Pdf);
        assert!(parsed.lossless);
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

    #[test]
    fn test_cli_parse_book_scan_runtime_options() {
        use img2pdf::book_scan::{JpegSampling, PartialGrayscaleMode, SuperResolutionMode};
        use img2pdf::cli::{CliMode, parse_args};

        let args = vec![
            "book-scan".to_string(),
            "/in.pdf".to_string(),
            "/out.pdf".to_string(),
            "--normalize".to_string(),
            "off".to_string(),
            "--blank-dark-delta".to_string(),
            "30".to_string(),
            "--blank-max-edge-ratio".to_string(),
            "0.001".to_string(),
            "--crop-exclude-pages".to_string(),
            "1,158".to_string(),
            "--color-normalize".to_string(),
            "off".to_string(),
            "--ink-neutralize".to_string(),
            "on".to_string(),
            "--ink-neutralize-strength".to_string(),
            "80".to_string(),
            "--ink-neutralize-exclude-pages".to_string(),
            "1,22-23".to_string(),
            "--pre-stroke".to_string(),
            "on".to_string(),
            "--pre-stroke-strength".to_string(),
            "7".to_string(),
            "--superres".to_string(),
            "lanczos".to_string(),
            "--output-scale".to_string(),
            "3".to_string(),
            "--ai-scale".to_string(),
            "4".to_string(),
            "--gpu-workers".to_string(),
            "2".to_string(),
            "--stroke-strength".to_string(),
            "22".to_string(),
            "--tone-boost-strength".to_string(),
            "85".to_string(),
            "--partial-grayscale".to_string(),
            "force".to_string(),
            "--partial-grayscale-strength".to_string(),
            "70".to_string(),
            "--tone-color-global-threshold".to_string(),
            "0.02".to_string(),
            "--tone-color-tile-threshold".to_string(),
            "0.4".to_string(),
            "--tone-exclude-pages".to_string(),
            "1,10-12".to_string(),
            "--jpeg-quality".to_string(),
            "94".to_string(),
            "--jpeg-sampling".to_string(),
            "420".to_string(),
            "--preserve-position".to_string(),
            "off".to_string(),
        ];
        let parsed = parse_args(&args).expect("Should parse book-scan args");
        assert_eq!(parsed.mode, CliMode::BookScan);
        assert!(parsed.parse_error.is_none());
        let config = parsed.book_scan.unwrap();
        assert!(!config.normalize_illumination);
        assert_eq!(config.blank_dark_delta, 30);
        assert_eq!(config.blank_max_edge_ratio, 0.001);
        assert_eq!(config.crop_exclude_pages.len(), 2);
        assert!(!config.color_normalization_enabled);
        assert!(config.ink_neutralization_enabled);
        assert_eq!(config.ink_neutralization_strength, 80);
        assert_eq!(config.ink_neutralization_exclude_pages.len(), 2);
        assert!(config.pre_stroke_enabled);
        assert_eq!(config.pre_stroke_strength, 7);
        assert_eq!(config.super_resolution, SuperResolutionMode::Lanczos);
        assert_eq!(config.superres_output_scale, 3);
        assert_eq!(config.superres_ai_scale, 4);
        assert_eq!(config.gpu_workers, 2);
        assert_eq!(config.stroke_strength, 22);
        assert_eq!(config.tone_boost_strength, 85);
        assert_eq!(config.partial_grayscale, PartialGrayscaleMode::Force);
        assert_eq!(config.partial_grayscale_strength, 70);
        assert_eq!(config.tone_color_global_threshold, 0.02);
        assert_eq!(config.tone_color_tile_threshold, 0.4);
        assert_eq!(config.tone_exclude_pages.len(), 2);
        assert_eq!(config.jpeg_quality, 94);
        assert_eq!(config.jpeg_sampling, JpegSampling::S420);
        assert!(!config.preserve_position);
    }

    #[test]
    fn test_cli_book_scan_rejects_bad_runtime_value() {
        use img2pdf::cli::parse_args;
        let args = vec![
            "book-scan".to_string(),
            "/in.pdf".to_string(),
            "--gpu-workers".to_string(),
            "many".to_string(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.parse_error.unwrap().contains("--gpu-workers"));
    }

    #[test]
    fn test_cli_book_scan_accepts_legacy_scale_aliases() {
        use img2pdf::cli::parse_args;
        let args = vec![
            "book-scan".to_string(),
            "/in.pdf".to_string(),
            "--scale".to_string(),
            "2".to_string(),
            "--inference-scale".to_string(),
            "4".to_string(),
        ];
        let config = parse_args(&args).unwrap().book_scan.unwrap();
        assert_eq!(config.superres_output_scale, 2);
        assert_eq!(config.superres_ai_scale, 4);
    }

    #[test]
    fn test_cli_no_scantailor_disables_dependent_features() {
        use img2pdf::cli::parse_args;
        let args = vec![
            "book-scan".to_string(),
            "/in.pdf".to_string(),
            "--no-scantailor".to_string(),
        ];
        let config = parse_args(&args).unwrap().book_scan.unwrap();
        assert!(!config.scantailor_enabled);
        assert!(!config.crop_enabled);
        assert!(!config.normalize_illumination);
        assert!(!config.deskew_enabled);
        assert!(!config.dewarp_enabled);
    }

    #[test]
    fn test_cli_book_scan_rejects_unknown_and_missing_options() {
        use img2pdf::cli::parse_args;
        let args = vec![
            "book-scan".to_string(),
            "/in.pdf".to_string(),
            "--gpu-workers".to_string(),
            "--unknown-setting".to_string(),
        ];
        let error = parse_args(&args).unwrap().parse_error.unwrap();
        assert!(error.contains("--gpu-workers に値がありません"));
        assert!(error.contains("不明なオプションです: --unknown-setting"));
    }

    // ─── jpegtran 最小疎通 ─────────────────────────────────────────────
    // jpegtran バイナリが配置されていない場合はスキップする。

    #[test]
    fn test_jpegtran_lossless_minimal_smoke() {
        let Some(jpegtran) = ImageProcessor::find_jpegtran() else {
            eprintln!("jpegtran が見つかりません。テストをスキップします。");
            return;
        };

        // 単色の小さい JPEG を作成
        let img: RgbImage = ImageBuffer::from_pixel(32, 32, Rgb([220, 220, 220]));
        let mut input_jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut input_jpeg, 80)
            .encode_image(&img)
            .expect("failed to create input jpeg");

        let mut child = Command::new(jpegtran)
            .args(["-copy", "none", "-optimize"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to run jpegtran");
        let stdin = child.stdin.take().expect("failed to open jpegtran stdin");
        let out = std::thread::scope(|scope| {
            let input = &input_jpeg;
            let writer = scope.spawn(move || {
                let mut stdin = stdin;
                stdin.write_all(input)
            });
            let out = child
                .wait_with_output()
                .expect("failed to read jpegtran output");
            writer
                .join()
                .expect("failed to join jpegtran stdin writer")
                .expect("failed to write jpegtran stdin");
            out
        });

        assert!(
            out.status.success(),
            "jpegtran failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!out.stdout.is_empty(), "jpegtran output is empty");
        assert!(
            out.stdout.starts_with(&[0xFF, 0xD8]),
            "jpegtran output is not JPEG"
        );
    }
}
