use super::config::{BookScanConfig, PartialGrayscaleMode};
use super::progress::{ProgressCallback, ProgressCounter};
use image::RgbImage;
use rayon::prelude::*;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default)]
pub struct PageTone {
    pub monochrome: bool,
    pub excluded: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ToneProfile {
    ink: [f32; 3],
    paper: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct TonePlan {
    pub pages: Vec<PageTone>,
    pub profile: Option<ToneProfile>,
}

#[derive(Debug, Clone, Copy)]
struct PageStats {
    ink: [f32; 3],
    paper: [f32; 3],
}

pub fn analyze_pages(
    config: &BookScanConfig,
    inputs: &[(usize, PathBuf)],
    blank: &[bool],
    page_numbers: &[usize],
    progress: &ProgressCallback<'_>,
) -> Result<TonePlan, String> {
    let enabled =
        config.tone_boost_enabled || config.partial_grayscale != PartialGrayscaleMode::Off;
    if !enabled {
        return Ok(TonePlan {
            pages: vec![PageTone::default(); inputs.len()],
            profile: None,
        });
    }

    let counter = ProgressCounter::new("色・階調解析", inputs.len(), progress);
    let analyzed = inputs
        .par_iter()
        .map(|(index, path)| {
            let excluded = blank[*index] || config.tone_excluded(page_numbers[*index]);
            if excluded {
                counter.advance();
                return Ok((
                    *index,
                    PageTone {
                        monochrome: false,
                        excluded,
                    },
                    None,
                ));
            }
            let image = image::open(path)
                .map_err(|e| format!("{}を色・階調解析用に開けません: {e}", path.display()))?
                .to_rgb8();
            let (global, tile) = color_ratios(&image);
            let monochrome = global < config.tone_color_global_threshold
                && tile < config.tone_color_tile_threshold;
            let stats = monochrome.then(|| page_stats(&image));
            counter.advance();
            Ok((
                *index,
                PageTone {
                    monochrome,
                    excluded: false,
                },
                stats,
            ))
        })
        .collect::<Vec<Result<_, String>>>();

    let mut pages = vec![PageTone::default(); inputs.len()];
    let mut stats = Vec::new();
    for result in analyzed {
        let (index, page, page_stats) = result?;
        pages[index] = page;
        if let Some(page_stats) = page_stats {
            stats.push(page_stats);
        }
    }
    let profile = aggregate_stats(&stats);
    let monochrome = pages.iter().filter(|page| page.monochrome).count();
    let excluded = pages.iter().filter(|page| page.excluded).count();
    let color = pages.len().saturating_sub(monochrome + excluded);
    eprintln!(
        "本モード: tone解析 白黒本文 {monochrome} / カラー保護 {color} / 除外・空白 {excluded}ページ"
    );
    Ok(TonePlan { pages, profile })
}

/// 1/8縮小とblurでJPEG色縁を平均化し、ページ全体と16x16局所tileの色面積を測る。
fn color_ratios(source: &RgbImage) -> (f32, f32) {
    if source.width() == 0 || source.height() == 0 {
        return (0.0, 0.0);
    }
    let width = (source.width() / 8).max(1);
    let height = (source.height() / 8).max(1);
    let small =
        image::imageops::resize(source, width, height, image::imageops::FilterType::Triangle);
    let small = image::imageops::blur(&small, 1.0);
    const GRID: usize = 16;
    let mut colored = 0usize;
    let mut tile_colored = [0usize; GRID * GRID];
    let mut tile_total = [0usize; GRID * GRID];
    for (x, y, pixel) in small.enumerate_pixels() {
        let rgb = pixel.0;
        let maximum = *rgb.iter().max().unwrap_or(&0);
        let minimum = *rgb.iter().min().unwrap_or(&0);
        let chroma = maximum - minimum;
        let luma = luminance(rgb);
        let is_colored =
            luma < 245 && chroma >= 18 && u32::from(chroma) * 255 >= u32::from(maximum.max(1)) * 28;
        let tx = ((x as usize * GRID) / small.width() as usize).min(GRID - 1);
        let ty = ((y as usize * GRID) / small.height() as usize).min(GRID - 1);
        let tile = ty * GRID + tx;
        tile_total[tile] += 1;
        if is_colored {
            colored += 1;
            tile_colored[tile] += 1;
        }
    }
    let total = (small.width() as usize * small.height() as usize).max(1);
    let max_tile = tile_colored
        .iter()
        .zip(tile_total)
        .filter(|(_, total)| *total > 0)
        .map(|(colored, total)| *colored as f32 / total as f32)
        .fold(0.0f32, f32::max);
    (colored as f32 / total as f32, max_tile)
}

fn page_stats(source: &RgbImage) -> PageStats {
    let mut histogram = [0usize; 256];
    let mut sampled = 0usize;
    for y in (0..source.height()).step_by(4) {
        for x in (0..source.width()).step_by(4) {
            histogram[luminance(source.get_pixel(x, y).0) as usize] += 1;
            sampled += 1;
        }
    }
    let low_target = (sampled as f32 * 0.05).ceil() as usize;
    let high_target = (sampled as f32 * 0.95).floor() as usize;
    let mut cumulative = 0usize;
    let mut low = 0u8;
    let mut high = 255u8;
    let mut low_found = false;
    for (value, count) in histogram.iter().enumerate() {
        cumulative += count;
        if !low_found && cumulative >= low_target {
            low = value as u8;
            low_found = true;
        }
        if cumulative >= high_target {
            high = value as u8;
            break;
        }
    }

    let mut ink_sum = [0u64; 3];
    let mut paper_sum = [0u64; 3];
    let mut ink_count = 0u64;
    let mut paper_count = 0u64;
    for y in (0..source.height()).step_by(4) {
        for x in (0..source.width()).step_by(4) {
            let rgb = source.get_pixel(x, y).0;
            let luma = luminance(rgb);
            if luma <= low {
                for channel in 0..3 {
                    ink_sum[channel] += u64::from(rgb[channel]);
                }
                ink_count += 1;
            }
            if luma >= high {
                for channel in 0..3 {
                    paper_sum[channel] += u64::from(rgb[channel]);
                }
                paper_count += 1;
            }
        }
    }
    let average = |sum: [u64; 3], count: u64| {
        let count = count.max(1) as f32;
        [
            sum[0] as f32 / count,
            sum[1] as f32 / count,
            sum[2] as f32 / count,
        ]
    };
    PageStats {
        ink: average(ink_sum, ink_count),
        paper: average(paper_sum, paper_count),
    }
}

fn aggregate_stats(stats: &[PageStats]) -> Option<ToneProfile> {
    if stats.is_empty() {
        return None;
    }
    let paper_luma = stats
        .iter()
        .map(|stats| float_luminance(stats.paper))
        .collect::<Vec<_>>();
    let center = median(paper_luma.clone());
    let mad = median(
        paper_luma
            .iter()
            .map(|value| (value - center).abs())
            .collect(),
    );
    let retained = stats
        .iter()
        .zip(paper_luma)
        .filter(|(_, value)| mad == 0.0 || (*value - center).abs() <= 3.0 * mad)
        .map(|(stats, _)| *stats)
        .collect::<Vec<_>>();
    let retained = if retained.is_empty() {
        stats
    } else {
        &retained
    };
    let mut ink = [0.0; 3];
    let mut paper = [0.0; 3];
    for channel in 0..3 {
        ink[channel] = median(retained.iter().map(|s| s.ink[channel]).collect());
        paper[channel] = median(retained.iter().map(|s| s.paper[channel]).collect());
    }
    let contrast = float_luminance(paper) - float_luminance(ink);
    (contrast >= 40.0).then_some(ToneProfile { ink, paper })
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

pub fn apply_boost(source: &RgbImage, profile: ToneProfile, strength: u8) -> RgbImage {
    if strength == 0 {
        return source.clone();
    }
    let blend = f32::from(strength.min(100)) / 100.0;
    let mut output = source.clone();
    for (original, corrected) in source
        .as_raw()
        .chunks_exact(3)
        .zip(output.as_mut().chunks_exact_mut(3))
    {
        let mut mapped = [0.0f32; 3];
        for channel in 0..3 {
            let scale =
                (255.0 / (profile.paper[channel] - profile.ink[channel]).max(1.0)).clamp(0.8, 4.0);
            mapped[channel] =
                ((f32::from(original[channel]) - profile.ink[channel]) * scale).clamp(0.0, 255.0);
        }
        let mapped_luma = float_luminance(mapped);
        let chroma = mapped.iter().copied().fold(f32::MIN, f32::max)
            - mapped.iter().copied().fold(f32::MAX, f32::min);
        let paper_weight =
            smoothstep(205.0, 248.0, mapped_luma) * ((55.0 - chroma) / 35.0).clamp(0.0, 1.0);
        for channel in 0..3 {
            let contrasted = ((mapped[channel] - 128.0) * 1.08 + 128.0).clamp(0.0, 255.0);
            let transformed = contrasted * (1.0 - paper_weight) + 255.0 * paper_weight;
            corrected[channel] = (f32::from(original[channel]) * (1.0 - blend)
                + transformed * blend)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    output
}

/// 暗い文字では完全に、明るいアンチエイリアスでは徐々に色差だけを除く。
/// 輝度を維持し、245以上の紙色には触れない。
pub fn partial_grayscale(source: &RgbImage, strength: u8) -> RgbImage {
    if strength == 0 {
        return source.clone();
    }
    let strength = f32::from(strength.min(100)) / 100.0;
    let mut output = source.clone();
    for (original, corrected) in source
        .as_raw()
        .chunks_exact(3)
        .zip(output.as_mut().chunks_exact_mut(3))
    {
        let luma = luminance([original[0], original[1], original[2]]);
        let darkness = if luma <= 210 {
            1.0
        } else if luma >= 245 {
            0.0
        } else {
            1.0 - smoothstep(210.0, 245.0, f32::from(luma))
        };
        let weight = strength * darkness;
        for channel in 0..3 {
            corrected[channel] = (f32::from(original[channel]) * (1.0 - weight)
                + f32::from(luma) * weight)
                .round() as u8;
        }
    }
    output
}

pub fn should_apply_gray(mode: PartialGrayscaleMode, page: PageTone) -> bool {
    if page.excluded {
        return false;
    }
    match mode {
        PartialGrayscaleMode::Off => false,
        PartialGrayscaleMode::Auto => page.monochrome,
        PartialGrayscaleMode::Force => true,
    }
}

fn smoothstep(start: f32, end: f32, value: f32) -> f32 {
    let t = ((value - start) / (end - start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn luminance(rgb: [u8; 3]) -> u8 {
    ((54 * u32::from(rgb[0]) + 183 * u32::from(rgb[1]) + 19 * u32::from(rgb[2]) + 128) / 256) as u8
}

fn float_luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    #[test]
    fn partial_gray_removes_dark_blue_but_keeps_paper_color() {
        let mut input: RgbImage = ImageBuffer::from_pixel(2, 1, Rgb([250, 248, 245]));
        input.put_pixel(0, 0, Rgb([45, 55, 95]));
        let output = partial_grayscale(&input, 100);
        let dark = output.get_pixel(0, 0).0;
        assert_eq!(dark[0], dark[1]);
        assert_eq!(dark[1], dark[2]);
        assert_eq!(output.get_pixel(1, 0).0, [250, 248, 245]);
    }

    #[test]
    fn color_guard_detects_coherent_blue_area() {
        let mut input: RgbImage = ImageBuffer::from_pixel(256, 256, Rgb([250, 250, 250]));
        for y in 0..64 {
            for x in 0..64 {
                input.put_pixel(x, y, Rgb([30, 90, 210]));
            }
        }
        let (global, tile) = color_ratios(&input);
        assert!(global > 0.01);
        assert!(tile > 0.30);
    }

    #[test]
    fn force_gray_still_respects_manual_exclusion() {
        assert!(!should_apply_gray(
            PartialGrayscaleMode::Force,
            PageTone {
                monochrome: false,
                excluded: true,
            }
        ));
    }
}
