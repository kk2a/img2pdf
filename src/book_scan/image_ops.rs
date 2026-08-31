use super::config::{BookScanConfig, JpegSampling};
use super::manifest::{BookScanStage, PageRecord};
use super::progress::{ProgressCallback, ProgressCounter};
use super::tone::{self, PageTone, ToneProfile};
use image::{DynamicImage, RgbImage};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};

pub fn prepare_for_superres(
    input: &Path,
    output: &Path,
    config: &BookScanConfig,
    neutralize_ink: bool,
) -> Result<PathBuf, String> {
    let mut image = image::open(input)
        .map_err(|e| format!("{}を前処理用に開けません: {e}", input.display()))?
        .to_rgb8();
    if config.color_normalization_enabled && config.color_normalization_strength > 0 {
        image = normalize_paper_color(
            &image,
            config.color_normalization_radius,
            config.color_normalization_strength,
        );
    }
    if neutralize_ink && config.ink_neutralization_strength > 0 {
        image = neutralize_black_ink(&image, config.ink_neutralization_strength);
    }
    if config.pre_stroke_enabled && config.pre_stroke_strength > 0 {
        image = minimum_blend(&image, config.pre_stroke_strength);
    }
    image
        .save(output)
        .map_err(|e| format!("{}へ前処理画像を保存できません: {e}", output.display()))?;
    Ok(output.to_path_buf())
}

/// 黒文字の内部にある低彩度な暗部を核として、そのごく近傍の色差だけを弱める。
///
/// ページ全体や「暗い画素すべて」を無彩色化すると濃紺の見出しまで壊れるため、
/// 黒インクらしい核から2px以内かつ明るすぎない画素だけを対象にする。輝度は
/// 保持するので、文字の太さやアンチエイリアスの形は変えない。
pub fn neutralize_black_ink(source: &RgbImage, strength: u8) -> RgbImage {
    let width = source.width();
    let height = source.height();
    if width == 0 || height == 0 || strength == 0 {
        return source.clone();
    }

    const MAX_DISTANCE: u8 = 3;
    let width = width as usize;
    let height = height as usize;
    let mut distance = vec![MAX_DISTANCE; width * height];

    // 実画像での中程度設定: luma < 0.74、RGB chroma < 0.16。
    for (i, rgb) in source.as_raw().chunks_exact(3).enumerate() {
        let luminance = ink_luminance(rgb);
        let maximum = *rgb.iter().max().unwrap_or(&0);
        let minimum = *rgb.iter().min().unwrap_or(&0);
        let chroma = maximum - minimum;
        if luminance < 189 && chroma < 41 {
            distance[i] = 0;
        }
    }

    // 半径2の菱形distance transform。固定13近傍を全画素で調べるより軽い。
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            if x > 0 {
                distance[i] = distance[i].min(distance[i - 1].saturating_add(1));
            }
            if y > 0 {
                distance[i] = distance[i].min(distance[i - width].saturating_add(1));
            }
        }
    }
    for y in (0..height).rev() {
        for x in (0..width).rev() {
            let i = y * width + x;
            if x + 1 < width {
                distance[i] = distance[i].min(distance[i + 1].saturating_add(1));
            }
            if y + 1 < height {
                distance[i] = distance[i].min(distance[i + width].saturating_add(1));
            }
        }
    }

    let strength = u32::from(strength.min(100));
    let mut output = source.clone();
    for ((original, corrected), distance) in source
        .as_raw()
        .chunks_exact(3)
        .zip(output.as_mut().chunks_exact_mut(3))
        .zip(distance)
    {
        if distance > 2 {
            continue;
        }
        let luminance = ink_luminance(original);
        if luminance >= 230 {
            continue;
        }
        let distance_weight = if distance < 2 { 100 } else { 75 };
        let edge_weight = if luminance <= 210 {
            100
        } else {
            u32::from(230 - luminance) * 5
        };
        let weight = (strength * distance_weight * edge_weight + 5_000) / 10_000;
        let gray = u32::from(luminance);
        for channel in 0..3 {
            corrected[channel] =
                ((u32::from(original[channel]) * (100 - weight) + gray * weight + 50) / 100) as u8;
        }
    }
    output
}

fn ink_luminance(rgb: &[u8]) -> u8 {
    ((54 * u32::from(rgb[0]) + 183 * u32::from(rgb[1]) + 19 * u32::from(rgb[2]) + 128) / 256) as u8
}

/// 明るく低彩度な紙面だけを、推定した低周波背景でRGBチャンネル別に補正する。
/// 色図版と黒い文字はmaskから外し、紙色・照明むら・薄い裏写りを主に軽減する。
pub fn normalize_paper_color(source: &RgbImage, radius: u32, strength: u8) -> RgbImage {
    if source.width() == 0 || source.height() == 0 || strength == 0 {
        return source.clone();
    }
    let factor = 8u32;
    let small_width = (source.width() / factor).max(8);
    let small_height = (source.height() / factor).max(8);
    let small = image::imageops::resize(
        source,
        small_width,
        small_height,
        image::imageops::FilterType::Triangle,
    );
    let blurred = image::imageops::blur(&small, (radius.max(1) as f32 / factor as f32).max(0.5));
    let background = image::imageops::resize(
        &blurred,
        source.width(),
        source.height(),
        image::imageops::FilterType::Triangle,
    );
    let strength = f32::from(strength.min(100)) / 100.0;
    let mut output = RgbImage::new(source.width(), source.height());
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let original = source.get_pixel(x, y).0;
        let bg = background.get_pixel(x, y).0;
        let maximum = *original.iter().max().unwrap_or(&0);
        let minimum = *original.iter().min().unwrap_or(&0);
        let chroma = f32::from(maximum - minimum);
        let luminance = 0.2126 * f32::from(original[0])
            + 0.7152 * f32::from(original[1])
            + 0.0722 * f32::from(original[2]);
        let neutral_weight = ((32.0 - chroma) / 24.0).clamp(0.0, 1.0);
        let light_weight = ((luminance - 128.0) / 80.0).clamp(0.0, 1.0);
        let weight = strength * neutral_weight * light_weight;
        let mut corrected = [0u8; 3];
        for channel in 0..3 {
            let normalized = f32::from(original[channel]) * 255.0 / f32::from(bg[channel].max(1));
            corrected[channel] = (f32::from(original[channel]) * (1.0 - weight)
                + normalized.clamp(0.0, 255.0) * weight)
                .round() as u8;
        }
        *pixel = image::Rgb(corrected);
    }
    output
}

pub fn save_lanczos(input: &Path, output: &Path, scale: u32) -> Result<(), String> {
    let image = image::open(input).map_err(|e| format!("画像を開けません: {e}"))?;
    let width = image
        .width()
        .checked_mul(scale)
        .ok_or("画像幅が大きすぎます")?;
    let height = image
        .height()
        .checked_mul(scale)
        .ok_or("画像高さが大きすぎます")?;
    let resized = image.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
    resized
        .save(output)
        .map_err(|e| format!("Lanczos画像を保存できません: {e}"))
}

pub fn encode_pages(
    config: &BookScanConfig,
    work_dir: &Path,
    pages: &mut [PageRecord],
    progress: &ProgressCallback<'_>,
) -> Result<(), String> {
    let output_dir = work_dir.join("jpeg");
    fs::create_dir_all(&output_dir)
        .map_err(|e| format!("JPEG出力フォルダを作成できません: {e}"))?;
    let inputs = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            if page.is_blank {
                Ok((index, page.source_path.clone()))
            } else {
                page.processed_path
                    .clone()
                    .or_else(|| page.crop_path.clone())
                    .ok_or_else(|| format!("{}の処理済み画像がありません", page.stem))
                    .map(|path| (index, path))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.cpu_workers)
        .build()
        .map_err(|e| format!("CPU worker poolを作成できません: {e}"))?;
    let blank = pages.iter().map(|page| page.is_blank).collect::<Vec<_>>();
    let page_numbers = pages
        .iter()
        .map(|page| page.page_number)
        .collect::<Vec<_>>();
    let tone_plan =
        pool.install(|| tone::analyze_pages(config, &inputs, &blank, &page_numbers, progress))?;
    let counter = ProgressCounter::new("JPEG化", inputs.len(), progress);
    let results = pool.install(|| {
        inputs
            .par_iter()
            .map(|(index, input)| {
                let output = output_dir.join(format!("{}.jpg", pages[*index].stem));
                encode_page(
                    input,
                    &output,
                    config,
                    tone_plan.pages[*index],
                    tone_plan.profile,
                    (!pages[*index].is_blank && config.stroke_enabled)
                        .then_some(config.stroke_strength),
                )?;
                counter.advance();
                Ok::<_, String>((*index, output))
            })
            .collect::<Vec<_>>()
    });

    for result in results {
        let (index, output) = result?;
        let dimensions = image::image_dimensions(&output)
            .map_err(|e| format!("生成JPEGを検証できません: {e}"))?;
        let expected = if pages[index].is_blank {
            (pages[index].source_width, pages[index].source_height)
        } else {
            let expected_scale = config.effective_output_scale();
            (
                pages[index].crop_width * expected_scale,
                pages[index].crop_height * expected_scale,
            )
        };
        if dimensions != expected {
            return Err(format!(
                "JPEG寸法が不一致です: {} expected={}x{} actual={}x{}",
                pages[index].stem, expected.0, expected.1, dimensions.0, dimensions.1
            ));
        }
        pages[index].jpeg_path = Some(output);
        pages[index].stage = BookScanStage::Encoded;
        pages[index].error = None;
    }
    Ok(())
}

fn encode_page(
    input: &Path,
    output: &Path,
    config: &BookScanConfig,
    page_tone: PageTone,
    tone_profile: Option<ToneProfile>,
    stroke_strength: Option<u8>,
) -> Result<(), String> {
    let mut image = image::open(input)
        .map_err(|e| format!("{}を開けません: {e}", input.display()))?
        .to_rgb8();
    if config.tone_boost_enabled
        && page_tone.monochrome
        && !page_tone.excluded
        && let Some(profile) = tone_profile
    {
        image = tone::apply_boost(&image, profile, config.tone_boost_strength);
    }
    if let Some(strength) = stroke_strength.filter(|strength| *strength > 0) {
        image = minimum_blend(&image, strength);
    }
    if tone::should_apply_gray(config.partial_grayscale, page_tone) {
        image = tone::partial_grayscale(&image, config.partial_grayscale_strength);
    }
    encode_jpeg(&image, output, config.jpeg_quality, config.jpeg_sampling)
}

pub fn minimum_blend(source: &RgbImage, strength: u8) -> RgbImage {
    let width = source.width();
    let height = source.height();
    if width == 0 || height == 0 || strength == 0 {
        return source.clone();
    }
    let original_weight = 100u32 - u32::from(strength.min(100));
    let minimum_weight = u32::from(strength.min(100));
    // 3x3 minimumは水平・垂直の2passに分離できる。9近傍を毎回走査するより
    // full bookで大幅に軽く、結果は同一になる。
    let mut horizontal = vec![[u8::MAX; 3]; (width as usize) * (height as usize)];
    for y in 0..height {
        for x in 0..width {
            let x0 = x.saturating_sub(1);
            let x1 = (x + 1).min(width - 1);
            let mut minimum = [u8::MAX; 3];
            for sample_x in x0..=x1 {
                let pixel = source.get_pixel(sample_x, y).0;
                for channel in 0..3 {
                    minimum[channel] = minimum[channel].min(pixel[channel]);
                }
            }
            horizontal[(y * width + x) as usize] = minimum;
        }
    }

    let mut output = RgbImage::new(width, height);
    for y in 0..height {
        let y0 = y.saturating_sub(1);
        let y1 = (y + 1).min(height - 1);
        for x in 0..width {
            let mut minimum = [u8::MAX; 3];
            for sample_y in y0..=y1 {
                let pixel = horizontal[(sample_y * width + x) as usize];
                for channel in 0..3 {
                    minimum[channel] = minimum[channel].min(pixel[channel]);
                }
            }
            let original = source.get_pixel(x, y).0;
            let mut blended = [0u8; 3];
            for channel in 0..3 {
                blended[channel] = ((u32::from(original[channel]) * original_weight
                    + u32::from(minimum[channel]) * minimum_weight
                    + 50)
                    / 100) as u8;
            }
            output.put_pixel(x, y, image::Rgb(blended));
        }
    }
    output
}

fn encode_jpeg(
    image: &RgbImage,
    output: &Path,
    quality: u8,
    sampling: JpegSampling,
) -> Result<(), String> {
    let width = u16::try_from(image.width()).map_err(|_| "JPEGの幅が65535を超えています")?;
    let height = u16::try_from(image.height()).map_err(|_| "JPEGの高さが65535を超えています")?;
    let file = fs::File::create(output)
        .map_err(|e| format!("{}を作成できません: {e}", output.display()))?;
    let mut encoder = Encoder::new(file, quality);
    encoder.set_sampling_factor(match sampling {
        JpegSampling::S444 => SamplingFactor::R_4_4_4,
        JpegSampling::S422 => SamplingFactor::R_4_2_2,
        JpegSampling::S420 => SamplingFactor::R_4_2_0,
    });
    encoder
        .encode(image.as_raw(), width, height, ColorType::Rgb)
        .map_err(|e| format!("JPEGエンコードに失敗しました: {e}"))
}

pub fn convert_to_png(input: &Path, output: &Path) -> Result<PathBuf, String> {
    let image: DynamicImage =
        image::open(input).map_err(|e| format!("{}を開けません: {e}", input.display()))?;
    image
        .save(output)
        .map_err(|e| format!("{}を保存できません: {e}", output.display()))?;
    Ok(output.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    #[test]
    fn minimum_15_darkens_only_near_dark_pixels() {
        let mut input: RgbImage = ImageBuffer::from_pixel(3, 3, Rgb([255, 255, 255]));
        input.put_pixel(1, 1, Rgb([0, 0, 0]));
        let output = minimum_blend(&input, 15);
        assert_eq!(output.get_pixel(1, 1).0, [0, 0, 0]);
        assert_eq!(output.get_pixel(0, 0).0, [217, 217, 217]);
    }

    #[test]
    fn minimum_zero_is_identity() {
        let input: RgbImage = ImageBuffer::from_pixel(2, 2, Rgb([10, 20, 30]));
        assert_eq!(minimum_blend(&input, 0), input);
    }

    #[test]
    fn ink_neutralization_removes_fringe_next_to_black_but_preserves_blue() {
        let mut input: RgbImage = ImageBuffer::from_pixel(11, 7, Rgb([255, 255, 255]));
        input.put_pixel(2, 3, Rgb([35, 34, 36]));
        input.put_pixel(3, 3, Rgb([95, 70, 140]));
        input.put_pixel(8, 3, Rgb([20, 75, 190]));

        let output = neutralize_black_ink(&input, 100);
        let fringe = output.get_pixel(3, 3).0;
        let chroma = *fringe.iter().max().unwrap() - *fringe.iter().min().unwrap();
        assert!(chroma < 15);
        assert_eq!(output.get_pixel(8, 3).0, [20, 75, 190]);
    }

    #[test]
    fn ink_neutralization_zero_is_identity() {
        let input: RgbImage = ImageBuffer::from_pixel(2, 2, Rgb([40, 50, 80]));
        assert_eq!(neutralize_black_ink(&input, 0), input);
    }

    #[test]
    fn paper_normalization_whitens_neutral_paper_but_preserves_color() {
        let mut input: RgbImage = ImageBuffer::from_pixel(64, 64, Rgb([220, 216, 210]));
        input.put_pixel(10, 10, Rgb([20, 20, 20]));
        input.put_pixel(20, 20, Rgb([20, 90, 220]));
        let output = normalize_paper_color(&input, 16, 100);
        assert!(output.get_pixel(40, 40).0[0] > 230);
        assert_eq!(output.get_pixel(10, 10).0, [20, 20, 20]);
        assert_eq!(output.get_pixel(20, 20).0, [20, 90, 220]);
    }
}
