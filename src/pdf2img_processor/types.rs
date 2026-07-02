/// pdf2img の出力画像形式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputImageFormat {
    /// JPEG は無劣化抽出し、それ以外は PNG でレンダリングする。
    AutoLossless,
    Jpeg,
    Png,
}

impl OutputImageFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "auto" | "lossless" => Some(Self::AutoLossless),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::AutoLossless | Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    pub(super) fn pdftoppm_flag(self) -> &'static str {
        match self {
            Self::AutoLossless | Self::Png => "-png",
            Self::Jpeg => "-jpeg",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PdfImageInfo {
    pub(super) page: usize,
    pub(super) enc: String,
}
