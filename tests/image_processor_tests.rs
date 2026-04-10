/// 画像プロセッサ単体テスト
///
/// RUST_MIGRATION_GUIDE.md §13 テスト項目チェックリストに対応
#[cfg(test)]
mod tests {
    use img2pdf::image_processor::{calc_worker_threads, ImageProcessor};
    use img2pdf::utils::constants::A4_RATIO;

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
}
