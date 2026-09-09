use super::config::BookScanConfig;
use super::manifest::{BlankPageMetrics, PageRecord};
use image::imageops::FilterType;
use rayon::prelude::*;

const PREVIEW_SIZE: u32 = 256;
const BORDER: u32 = 13; // 外周約5%は撮影端・綴じ影の影響を受けやすいので除外する。

pub fn classify_pages(config: &BookScanConfig, pages: &mut [PageRecord]) -> Result<usize, String> {
    if !config.blank_detection_enabled {
        for page in pages {
            page.is_blank = false;
            page.blank_metrics = None;
        }
        return Ok(0);
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.cpu_workers)
        .build()
        .map_err(|e| format!("空白判定worker poolを作成できません: {e}"))?;
    let results = pool.install(|| {
        pages
            .par_iter()
            .map(|page| measure(page, config))
            .collect::<Vec<_>>()
    });

    let mut blank_count = 0;
    for (page, result) in pages.iter_mut().zip(results) {
        let metrics = result?;
        page.is_blank = metrics.dark_ratio <= config.blank_max_dark_ratio
            && metrics.edge_ratio <= config.blank_max_edge_ratio;
        page.blank_metrics = Some(metrics);
        blank_count += usize::from(page.is_blank);
    }
    Ok(blank_count)
}

fn measure(page: &PageRecord, config: &BookScanConfig) -> Result<BlankPageMetrics, String> {
    let source = image::open(&page.source_path)
        .map_err(|e| {
            format!(
                "{}を空白判定用に開けません: {e}",
                page.source_path.display()
            )
        })?
        .to_luma8();
    let preview =
        image::imageops::resize(&source, PREVIEW_SIZE, PREVIEW_SIZE, FilterType::Triangle);

    let mut histogram = [0u32; 256];
    for y in BORDER..(PREVIEW_SIZE - BORDER) {
        for x in BORDER..(PREVIEW_SIZE - BORDER) {
            histogram[usize::from(preview.get_pixel(x, y).0[0])] += 1;
        }
    }
    let total = (PREVIEW_SIZE - BORDER * 2).pow(2);
    let background_level = percentile(&histogram, total, 90);
    let dark_cutoff = background_level.saturating_sub(config.blank_dark_delta);
    let mut dark = 0u32;
    let mut edge = 0u32;
    for y in BORDER..(PREVIEW_SIZE - BORDER) {
        for x in BORDER..(PREVIEW_SIZE - BORDER) {
            let value = preview.get_pixel(x, y).0[0];
            dark += u32::from(value < dark_cutoff);
            let left = preview.get_pixel(x.saturating_sub(1), y).0[0];
            let above = preview.get_pixel(x, y.saturating_sub(1)).0[0];
            let gradient = value.abs_diff(left).max(value.abs_diff(above));
            edge += u32::from(gradient >= config.blank_edge_threshold);
        }
    }

    Ok(BlankPageMetrics {
        background_level,
        dark_ratio: dark as f32 / total as f32,
        edge_ratio: edge as f32 / total as f32,
    })
}

fn percentile(histogram: &[u32; 256], total: u32, percentile: u32) -> u8 {
    let target = (total * percentile).div_ceil(100);
    let mut seen = 0u32;
    for (value, count) in histogram.iter().enumerate() {
        seen += count;
        if seen >= target {
            return value as u8;
        }
    }
    u8::MAX
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book_scan::BookScanStage;
    use image::{GrayImage, Luma};
    use std::path::{Path, PathBuf};

    fn record(path: &Path) -> PageRecord {
        PageRecord {
            index: 0,
            page_number: 1,
            stem: "p0001".to_string(),
            source_path: path.to_path_buf(),
            source_width: 512,
            source_height: 512,
            is_blank: false,
            blank_metrics: None,
            crop_path: None,
            crop_width: 512,
            crop_height: 512,
            restore_x: 0.0,
            restore_y: 0.0,
            processed_path: None,
            jpeg_path: None,
            stage: BookScanStage::Extracted,
            attempts: 0,
            error: None,
        }
    }

    fn temporary(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("img2pdf-blank-{name}-{}.png", std::process::id()))
    }

    #[test]
    fn uniform_paper_is_blank_but_content_is_not() {
        let blank_path = temporary("uniform");
        GrayImage::from_pixel(512, 512, Luma([238]))
            .save(&blank_path)
            .unwrap();
        let mut blank = vec![record(&blank_path)];
        classify_pages(&BookScanConfig::default(), &mut blank).unwrap();
        assert!(blank[0].is_blank);

        let content_path = temporary("content");
        let mut content = GrayImage::from_pixel(512, 512, Luma([245]));
        for y in 220..250 {
            for x in 100..412 {
                content.put_pixel(x, y, Luma([30]));
            }
        }
        content.save(&content_path).unwrap();
        let mut content = vec![record(&content_path)];
        classify_pages(&BookScanConfig::default(), &mut content).unwrap();
        assert!(!content[0].is_blank);

        let _ = std::fs::remove_file(blank_path);
        let _ = std::fs::remove_file(content_path);
    }
}
