use serde::{Deserialize, Serialize};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuperResolutionMode {
    Off,
    Anime,
    Lanczos,
}

impl SuperResolutionMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "off" | "none" | "0" => Some(Self::Off),
            "anime" | "anime-x2" | "realesr-animevideov3" => Some(Self::Anime),
            "lanczos" | "lanczos3" => Some(Self::Lanczos),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JpegSampling {
    S444,
    S422,
    S420,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartialGrayscaleMode {
    Off,
    Auto,
    Force,
}

impl PartialGrayscaleMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "off" | "none" | "0" => Some(Self::Off),
            "auto" | "automatic" => Some(Self::Auto),
            "force" | "on" | "1" => Some(Self::Force),
            _ => None,
        }
    }
}

impl JpegSampling {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().replace(':', "").as_str() {
            "444" => Some(Self::S444),
            "422" => Some(Self::S422),
            "420" => Some(Self::S420),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRange {
    pub start: usize,
    pub end: usize,
}

impl PageRange {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if let Some((start, end)) = value.split_once('-') {
            let start = start.trim().parse().ok()?;
            let end = end.trim().parse().ok()?;
            (start >= 1 && end >= start).then_some(Self { start, end })
        } else {
            let page = value.parse().ok()?;
            (page >= 1).then_some(Self {
                start: page,
                end: page,
            })
        }
    }

    pub fn len(self) -> usize {
        if self.is_empty() {
            0
        } else {
            self.end - self.start + 1
        }
    }

    pub fn is_empty(self) -> bool {
        self.end < self.start
    }

    pub fn parse_list(value: &str) -> Option<Vec<Self>> {
        if value.trim().is_empty() {
            return Some(Vec::new());
        }
        value
            .split(',')
            .map(|part| Self::parse(part.trim()))
            .collect()
    }

    pub fn format_list(ranges: &[Self]) -> String {
        ranges
            .iter()
            .map(|range| {
                if range.start == range.end {
                    range.start.to_string()
                } else {
                    format!("{}-{}", range.start, range.end)
                }
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookScanConfig {
    pub input_path: PathBuf,
    pub output_pdf: PathBuf,
    pub work_dir: Option<PathBuf>,
    pub pages: Option<PageRange>,

    #[serde(default = "default_true")]
    pub blank_detection_enabled: bool,
    #[serde(default = "default_blank_dark_delta")]
    pub blank_dark_delta: u8,
    #[serde(default = "default_blank_max_ratio")]
    pub blank_max_dark_ratio: f32,
    #[serde(default = "default_blank_edge_threshold")]
    pub blank_edge_threshold: u8,
    #[serde(default = "default_blank_max_ratio")]
    pub blank_max_edge_ratio: f32,

    pub scantailor_enabled: bool,
    pub crop_enabled: bool,
    #[serde(default = "default_crop_exclude_pages")]
    pub crop_exclude_pages: Vec<PageRange>,
    pub normalize_illumination: bool,
    #[serde(default = "default_true")]
    pub color_normalization_enabled: bool,
    #[serde(default = "default_color_normalization_strength")]
    pub color_normalization_strength: u8,
    #[serde(default = "default_color_normalization_radius")]
    pub color_normalization_radius: u32,
    #[serde(default)]
    pub ink_neutralization_enabled: bool,
    #[serde(default = "default_ink_neutralization_strength")]
    pub ink_neutralization_strength: u8,
    #[serde(default = "default_ink_neutralization_exclude_pages")]
    pub ink_neutralization_exclude_pages: Vec<PageRange>,
    pub deskew_enabled: bool,
    pub dewarp_enabled: bool,
    pub margins: f32,
    pub dpi: u32,
    pub output_dpi: u32,
    pub despeckle: f32,
    pub page_detection_tolerance: f32,

    pub super_resolution: SuperResolutionMode,
    /// ScanTailor出力に対する、PDFへ埋め込む最終画像の画素倍率。
    #[serde(default = "default_superres_output_scale", alias = "superres_scale")]
    pub superres_output_scale: u32,
    /// Real-ESRGANが内部で生成する倍率。0は旧設定との互換用auto。
    #[serde(
        default = "default_superres_ai_scale",
        alias = "superres_inference_scale"
    )]
    pub superres_ai_scale: u32,
    pub superres_model: String,
    pub gpu_workers: usize,
    pub cpu_workers: usize,
    pub tile_size: u32,
    pub gpu_id: i32,
    pub tta_enabled: bool,

    pub stroke_enabled: bool,
    pub stroke_strength: u8,
    /// 本全体から推定した黒点・白点で文字と紙面のコントラストを整える。
    #[serde(default = "default_true")]
    pub tone_boost_enabled: bool,
    #[serde(default = "default_tone_strength")]
    pub tone_boost_strength: u8,
    /// 暗い文字領域だけを無彩色化する。Autoはカラー頁を除外する。
    #[serde(default = "default_partial_grayscale")]
    pub partial_grayscale: PartialGrayscaleMode,
    #[serde(default = "default_tone_strength")]
    pub partial_grayscale_strength: u8,
    #[serde(default = "default_tone_color_global_threshold")]
    pub tone_color_global_threshold: f32,
    #[serde(default = "default_tone_color_tile_threshold")]
    pub tone_color_tile_threshold: f32,
    #[serde(default = "default_tone_exclude_pages")]
    pub tone_exclude_pages: Vec<PageRange>,
    /// 明示されたページだけを全体グレースケールの1成分JPEGにする。
    /// 空なら無効。意図的な色を失わないようAUTOモードは公開しない。
    #[serde(default)]
    pub grayscale_pages: Vec<PageRange>,
    #[serde(default)]
    pub pre_stroke_enabled: bool,
    #[serde(default = "default_pre_stroke_strength")]
    pub pre_stroke_strength: u8,
    pub jpeg_quality: u8,
    pub jpeg_sampling: JpegSampling,
    #[serde(default = "default_true")]
    pub preserve_position: bool,

    pub scantailor_path: Option<PathBuf>,
    pub realesrgan_path: Option<PathBuf>,
    pub realesrgan_model_dir: Option<PathBuf>,

    pub resume: bool,
    pub keep_work: bool,
}

impl Default for BookScanConfig {
    fn default() -> Self {
        Self {
            input_path: PathBuf::new(),
            output_pdf: PathBuf::new(),
            work_dir: None,
            pages: None,
            blank_detection_enabled: true,
            blank_dark_delta: default_blank_dark_delta(),
            blank_max_dark_ratio: default_blank_max_ratio(),
            blank_edge_threshold: default_blank_edge_threshold(),
            blank_max_edge_ratio: default_blank_max_ratio(),
            scantailor_enabled: true,
            crop_enabled: true,
            crop_exclude_pages: default_crop_exclude_pages(),
            normalize_illumination: true,
            color_normalization_enabled: true,
            color_normalization_strength: default_color_normalization_strength(),
            color_normalization_radius: default_color_normalization_radius(),
            ink_neutralization_enabled: false,
            ink_neutralization_strength: default_ink_neutralization_strength(),
            ink_neutralization_exclude_pages: default_ink_neutralization_exclude_pages(),
            deskew_enabled: false,
            dewarp_enabled: false,
            margins: 0.0,
            dpi: 300,
            output_dpi: 300,
            despeckle: 1.0,
            page_detection_tolerance: 0.1,
            super_resolution: SuperResolutionMode::Anime,
            // 元画像が既存img2pdfの標準A4（1654x2339）なら、その解像度と
            // ファイルサイズを維持しつつx2 AI結果を縮小して利用する。
            superres_output_scale: 1,
            superres_ai_scale: 2,
            superres_model: "realesr-animevideov3".to_string(),
            gpu_workers: 4,
            cpu_workers: 2,
            tile_size: 0,
            gpu_id: -1,
            tta_enabled: false,
            stroke_enabled: false,
            stroke_strength: 15,
            tone_boost_enabled: true,
            tone_boost_strength: default_tone_strength(),
            partial_grayscale: default_partial_grayscale(),
            partial_grayscale_strength: default_tone_strength(),
            tone_color_global_threshold: default_tone_color_global_threshold(),
            tone_color_tile_threshold: default_tone_color_tile_threshold(),
            tone_exclude_pages: default_tone_exclude_pages(),
            grayscale_pages: Vec::new(),
            pre_stroke_enabled: false,
            pre_stroke_strength: default_pre_stroke_strength(),
            jpeg_quality: 90,
            jpeg_sampling: JpegSampling::S444,
            preserve_position: true,
            scantailor_path: None,
            realesrgan_path: None,
            realesrgan_model_dir: None,
            resume: true,
            keep_work: false,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_blank_dark_delta() -> u8 {
    25
}

fn default_blank_edge_threshold() -> u8 {
    12
}

fn default_blank_max_ratio() -> f32 {
    0.0005
}

fn default_crop_exclude_pages() -> Vec<PageRange> {
    vec![PageRange { start: 1, end: 1 }]
}

fn default_color_normalization_strength() -> u8 {
    100
}

fn default_color_normalization_radius() -> u32 {
    40
}

fn default_ink_neutralization_strength() -> u8 {
    100
}

fn default_ink_neutralization_exclude_pages() -> Vec<PageRange> {
    vec![PageRange { start: 1, end: 1 }]
}

fn default_pre_stroke_strength() -> u8 {
    5
}

fn default_tone_strength() -> u8 {
    100
}

fn default_partial_grayscale() -> PartialGrayscaleMode {
    PartialGrayscaleMode::Off
}

fn default_tone_color_global_threshold() -> f32 {
    0.01
}

fn default_tone_color_tile_threshold() -> f32 {
    0.30
}

fn default_tone_exclude_pages() -> Vec<PageRange> {
    vec![PageRange { start: 1, end: 1 }]
}

fn default_superres_output_scale() -> u32 {
    1
}

fn default_superres_ai_scale() -> u32 {
    2
}

impl BookScanConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.input_path.as_os_str().is_empty() || !self.input_path.exists() {
            return Err(format!(
                "入力が見つかりません: {}",
                self.input_path.display()
            ));
        }
        if self.output_pdf.as_os_str().is_empty() {
            return Err("出力PDFを指定してください".to_string());
        }
        if self.partial_grayscale != PartialGrayscaleMode::Off {
            return Err(
                "暗部の自動・強制グレースケール化は廃止しました。1成分Grayにするページをgrayscale_pagesで明示してください"
                    .to_string(),
            );
        }
        if self.dpi == 0 || self.output_dpi == 0 {
            return Err("DPIには1以上を指定してください".to_string());
        }
        if self.blank_dark_delta == 0 || self.blank_edge_threshold == 0 {
            return Err(
                "空白判定のdark deltaとedge thresholdには1以上を指定してください".to_string(),
            );
        }
        if !(0.0..=0.1).contains(&self.blank_max_dark_ratio)
            || !(0.0..=0.1).contains(&self.blank_max_edge_ratio)
        {
            return Err("空白判定の最大比率には0.0–0.1を指定してください".to_string());
        }
        if !(1.0..=3.0).contains(&self.despeckle) {
            return Err("despeckleには1.0–3.0を指定してください".to_string());
        }
        if !(0.0..=1.0).contains(&self.page_detection_tolerance) {
            return Err("page detection toleranceには0.0–1.0を指定してください".to_string());
        }
        if !(1..=4).contains(&self.superres_output_scale) {
            return Err("最終画像倍率には1–4を指定してください".to_string());
        }
        if self.superres_ai_scale > 4 {
            return Err("AI推論倍率には0（auto）または1–4を指定してください".to_string());
        }
        if self.super_resolution == SuperResolutionMode::Anime
            && !(2..=4).contains(&self.resolved_ai_scale())
        {
            return Err("animeのAI推論倍率には2–4を指定してください".to_string());
        }
        if self.super_resolution == SuperResolutionMode::Anime
            && self.resolved_ai_scale() < self.superres_output_scale
        {
            return Err("AI推論倍率は最終画像倍率以上を指定してください".to_string());
        }
        if self.gpu_workers == 0 || self.gpu_workers > 8 {
            return Err("GPU worker数には1–8を指定してください".to_string());
        }
        if self.cpu_workers == 0 || self.cpu_workers > 64 {
            return Err("CPU worker数には1–64を指定してください".to_string());
        }
        if self.jpeg_quality == 0 || self.jpeg_quality > 100 {
            return Err("JPEG品質には1–100を指定してください".to_string());
        }
        if self.stroke_strength > 100 {
            return Err("文字太さには0–100を指定してください".to_string());
        }
        if self.pre_stroke_strength > 100
            || self.color_normalization_strength > 100
            || self.ink_neutralization_strength > 100
            || self.tone_boost_strength > 100
            || self.partial_grayscale_strength > 100
        {
            return Err("各種画像補正の強さには0–100を指定してください".to_string());
        }
        if !(0.0..=1.0).contains(&self.tone_color_global_threshold)
            || !(0.0..=1.0).contains(&self.tone_color_tile_threshold)
        {
            return Err("カラー頁判定の比率には0.0–1.0を指定してください".to_string());
        }
        if self.color_normalization_radius == 0 || self.color_normalization_radius > 256 {
            return Err("カラー紙面補正の半径には1–256を指定してください".to_string());
        }
        if !self.scantailor_enabled
            && (self.crop_enabled
                || self.normalize_illumination
                || self.deskew_enabled
                || self.dewarp_enabled)
        {
            return Err(
                "ScanTailorをOFFにする場合はcrop/normalize/deskew/dewarpもOFFにしてください"
                    .to_string(),
            );
        }
        Ok(())
    }

    pub fn resolved_work_dir(&self) -> PathBuf {
        self.work_dir.clone().unwrap_or_else(|| {
            let stem = self
                .output_pdf
                .file_stem()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or("output");
            let mut hasher = DefaultHasher::new();
            self.input_path.hash(&mut hasher);
            self.output_pdf.hash(&mut hasher);
            let fingerprint = hasher.finish();
            std::env::temp_dir().join(format!("img2pdf-book-{stem}-{fingerprint:016x}"))
        })
    }

    pub fn crop_excluded(&self, page_number: usize) -> bool {
        self.crop_enabled
            && self
                .crop_exclude_pages
                .iter()
                .any(|range| (range.start..=range.end).contains(&page_number))
    }

    pub fn ink_neutralization_excluded(&self, page_number: usize) -> bool {
        self.ink_neutralization_exclude_pages
            .iter()
            .any(|range| (range.start..=range.end).contains(&page_number))
    }

    pub fn tone_excluded(&self, page_number: usize) -> bool {
        self.tone_exclude_pages
            .iter()
            .any(|range| (range.start..=range.end).contains(&page_number))
    }

    pub fn grayscale_page(&self, page_number: usize) -> bool {
        self.grayscale_pages
            .iter()
            .any(|range| (range.start..=range.end).contains(&page_number))
    }

    /// Real-ESRGANへ渡す倍率。旧設定の0は、出力倍率以上かつ最低x2のauto。
    pub fn resolved_ai_scale(&self) -> u32 {
        if self.superres_ai_scale == 0 {
            self.superres_output_scale.max(2)
        } else {
            self.superres_ai_scale
        }
    }

    /// 処理後画像の倍率。超解像OFF時は設定値にかかわらずx1。
    pub fn effective_output_scale(&self) -> u32 {
        if self.super_resolution == SuperResolutionMode::Off {
            1
        } else {
            self.superres_output_scale
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_page_ranges() {
        assert_eq!(PageRange::parse("10-20").unwrap().len(), 11);
        assert_eq!(
            PageRange::parse("7").unwrap(),
            PageRange { start: 7, end: 7 }
        );
        assert!(PageRange::parse("20-10").is_none());
        assert_eq!(
            PageRange::parse_list("1,10-12,158").unwrap(),
            vec![
                PageRange { start: 1, end: 1 },
                PageRange { start: 10, end: 12 },
                PageRange {
                    start: 158,
                    end: 158
                }
            ]
        );
    }

    #[test]
    fn parses_runtime_modes() {
        assert_eq!(
            SuperResolutionMode::parse("off"),
            Some(SuperResolutionMode::Off)
        );
        assert_eq!(JpegSampling::parse("4:4:4"), Some(JpegSampling::S444));
        assert_eq!(
            PartialGrayscaleMode::parse("auto"),
            Some(PartialGrayscaleMode::Auto)
        );
    }

    #[test]
    fn default_ai_x2_keeps_original_output_resolution() {
        let config = BookScanConfig::default();
        assert_eq!(config.resolved_ai_scale(), 2);
        assert_eq!(config.effective_output_scale(), 1);
        assert!(config.tone_boost_enabled);
        assert_eq!(config.partial_grayscale, PartialGrayscaleMode::Off);
        assert!(config.grayscale_pages.is_empty());
        assert!(!config.stroke_enabled);
    }

    #[test]
    fn whole_page_grayscale_requires_an_explicit_page() {
        let config = BookScanConfig {
            grayscale_pages: PageRange::parse_list("3,8-10").unwrap(),
            ..BookScanConfig::default()
        };
        assert!(!config.grayscale_page(2));
        assert!(config.grayscale_page(3));
        assert!(config.grayscale_page(9));
        assert!(!config.grayscale_page(11));
    }

    #[test]
    fn default_work_directory_is_in_the_system_temp_directory() {
        let config = BookScanConfig {
            input_path: PathBuf::from("book.pdf"),
            output_pdf: PathBuf::from("result.pdf"),
            ..BookScanConfig::default()
        };
        assert!(config.resolved_work_dir().starts_with(std::env::temp_dir()));
        assert_eq!(config.resolved_work_dir(), config.resolved_work_dir());
    }

    #[test]
    fn legacy_auto_ai_scale_is_at_least_two() {
        let config = BookScanConfig {
            superres_output_scale: 1,
            superres_ai_scale: 0,
            ..BookScanConfig::default()
        };
        assert_eq!(config.resolved_ai_scale(), 2);
    }

    #[test]
    fn deserializes_legacy_scale_field_names() {
        let mut value = serde_json::to_value(BookScanConfig::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("superres_output_scale");
        object.remove("superres_ai_scale");
        object.insert("superres_scale".to_string(), serde_json::json!(2));
        object.insert("superres_inference_scale".to_string(), serde_json::json!(4));

        let config: BookScanConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.superres_output_scale, 2);
        assert_eq!(config.superres_ai_scale, 4);
    }

    #[test]
    fn old_saved_config_gets_safe_tone_defaults() {
        let mut value = serde_json::to_value(BookScanConfig::default()).unwrap();
        let object = value.as_object_mut().unwrap();
        for field in [
            "tone_boost_enabled",
            "tone_boost_strength",
            "partial_grayscale",
            "partial_grayscale_strength",
            "tone_color_global_threshold",
            "tone_color_tile_threshold",
            "tone_exclude_pages",
            "grayscale_pages",
        ] {
            object.remove(field);
        }
        let config: BookScanConfig = serde_json::from_value(value).unwrap();
        assert!(config.tone_boost_enabled);
        assert_eq!(config.tone_boost_strength, 100);
        assert_eq!(config.partial_grayscale, PartialGrayscaleMode::Off);
        assert_eq!(
            config.tone_exclude_pages,
            vec![PageRange { start: 1, end: 1 }]
        );
        assert!(config.grayscale_pages.is_empty());
    }
}
