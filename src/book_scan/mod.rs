//! 写真・スキャン由来の本を、紙面上の位置を保ったままPDF化するパイプライン。

mod blank;
mod config;
mod image_ops;
mod manifest;
mod pdf;
mod pipeline;
mod scantailor;
mod scheduler;
mod superres;
mod tools;

pub use config::{BookScanConfig, JpegSampling, PageRange, SuperResolutionMode};
pub use manifest::{BlankPageMetrics, BookScanManifest, BookScanStage, PageRecord};
pub use pdf::{A4_HEIGHT_PT, A4_WIDTH_PT, Placement, calculate_placement};
pub use pipeline::{BookScanProcessor, BookScanProgress, BookScanReport};
