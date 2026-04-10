/// 画像プロセッサ単体テスト
///
/// RUST_MIGRATION_GUIDE.md §13 テスト項目チェックリストに対応
#[cfg(test)]
mod tests {
    use img2pdf::image_processor::ImageProcessor;
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
}
