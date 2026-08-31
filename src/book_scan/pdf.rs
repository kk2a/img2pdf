use super::PageRecord;
use std::fs::{self, File};
use std::io::{BufWriter, Seek, Write};
use std::path::Path;

pub const A4_WIDTH_PT: f32 = 595.275_6;
pub const A4_HEIGHT_PT: f32 = 841.889_8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn calculate_placement(page: &PageRecord) -> Result<Placement, String> {
    if page.source_width == 0 || page.source_height == 0 {
        return Err(format!("{}の元キャンバス寸法が0です", page.stem));
    }
    let source_width = page.source_width as f32;
    let source_height = page.source_height as f32;
    let x = page.restore_x / source_width * A4_WIDTH_PT;
    let top = page.restore_y / source_height * A4_HEIGHT_PT;
    let width = page.crop_width as f32 / source_width * A4_WIDTH_PT;
    let height = page.crop_height as f32 / source_height * A4_HEIGHT_PT;
    Ok(Placement {
        x,
        y: A4_HEIGHT_PT - top - height,
        width,
        height,
    })
}

pub fn write_pdf(
    pages: &[PageRecord],
    output_path: &Path,
    preserve_position: bool,
) -> Result<(), String> {
    if pages.is_empty() {
        return Err("PDFへ書き出すページがありません".to_string());
    }
    if let Some(parent) = output_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|e| format!("PDF出力フォルダを作成できません: {e}"))?;
    }
    let temporary = output_path.with_extension("pdf.tmp");
    let file = File::create(&temporary).map_err(|e| format!("一時PDFを作成できません: {e}"))?;
    let mut writer = DirectPdfWriter::new(BufWriter::new(file))?;
    writer.write_document(pages, preserve_position)?;
    let mut file = writer.finish()?;
    file.flush()
        .map_err(|e| format!("PDF flushに失敗しました: {e}"))?;
    drop(file);
    if cfg!(windows) && output_path.exists() {
        fs::remove_file(output_path).map_err(|e| format!("旧PDFを置換できません: {e}"))?;
    }
    fs::rename(&temporary, output_path).map_err(|e| format!("PDF確定に失敗しました: {e}"))
}

struct DirectPdfWriter<W: Write + Seek> {
    output: W,
    offsets: Vec<u64>,
}

impl<W: Write + Seek> DirectPdfWriter<W> {
    fn new(mut output: W) -> Result<Self, String> {
        output
            .write_all(b"%PDF-1.7\n%\x80\x80\x80\x80\n")
            .map_err(|e| format!("PDF headerを書けません: {e}"))?;
        Ok(Self {
            output,
            offsets: vec![0],
        })
    }

    fn write_document(
        &mut self,
        pages: &[PageRecord],
        preserve_position: bool,
    ) -> Result<(), String> {
        let page_ids = (0..pages.len())
            .map(|index| 3 + index * 3)
            .collect::<Vec<_>>();
        self.write_object(1, b"<< /Type /Catalog /Pages 2 0 R >>\n")?;
        let kids = page_ids
            .iter()
            .map(|id| format!("{id} 0 R"))
            .collect::<Vec<_>>()
            .join(" ");
        self.write_object(
            2,
            format!(
                "<< /Type /Pages /Count {} /Kids [ {} ] >>\n",
                pages.len(),
                kids
            )
            .as_bytes(),
        )?;

        for (index, page) in pages.iter().enumerate() {
            let page_id = 3 + index * 3;
            let image_id = page_id + 1;
            let content_id = page_id + 2;
            let jpeg_path = page
                .jpeg_path
                .as_deref()
                .ok_or_else(|| format!("{}のJPEGがありません", page.stem))?;
            let jpeg = fs::read(jpeg_path)
                .map_err(|e| format!("{}を読めません: {e}", jpeg_path.display()))?;
            let color_space = match jpeg_components(&jpeg)? {
                1 => "/DeviceGray",
                3 => "/DeviceRGB",
                components => {
                    return Err(format!(
                        "未対応のJPEG成分数です: {} components={components}",
                        jpeg_path.display()
                    ));
                }
            };
            let (width, height) = image::image_dimensions(jpeg_path)
                .map_err(|e| format!("JPEG寸法を読めません: {e}"))?;
            let placement = if preserve_position {
                calculate_placement(page)?
            } else {
                calculate_centered_placement(page)?
            };

            let page_body = format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.4} {:.4}] /Resources << /XObject << /Im0 {} 0 R >> >> /Contents {} 0 R >>\n",
                A4_WIDTH_PT, A4_HEIGHT_PT, image_id, content_id
            );
            self.write_object(page_id, page_body.as_bytes())?;

            self.begin_object(image_id)?;
            write!(
                self.output,
                "<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {} /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
                width,
                height,
                color_space,
                jpeg.len()
            )
            .map_err(|e| format!("PDF image dictionaryを書けません: {e}"))?;
            self.output
                .write_all(&jpeg)
                .map_err(|e| format!("PDFへJPEGを書けません: {e}"))?;
            self.output
                .write_all(b"\nendstream\nendobj\n")
                .map_err(|e| format!("PDF image objectを閉じられません: {e}"))?;

            let content = format!(
                "q\n{:.6} 0 0 {:.6} {:.6} {:.6} cm\n/Im0 Do\nQ\n",
                placement.width, placement.height, placement.x, placement.y
            );
            self.begin_object(content_id)?;
            write!(self.output, "<< /Length {} >>\nstream\n", content.len())
                .map_err(|e| format!("PDF content dictionaryを書けません: {e}"))?;
            self.output
                .write_all(content.as_bytes())
                .and_then(|_| self.output.write_all(b"endstream\nendobj\n"))
                .map_err(|e| format!("PDF contentを書けません: {e}"))?;
        }
        Ok(())
    }

    fn begin_object(&mut self, id: usize) -> Result<(), String> {
        if id != self.offsets.len() {
            return Err(format!("PDF object IDが連続していません: {id}"));
        }
        let offset = self
            .output
            .stream_position()
            .map_err(|e| format!("PDF offsetを取得できません: {e}"))?;
        self.offsets.push(offset);
        writeln!(self.output, "{id} 0 obj").map_err(|e| format!("PDF objectを書けません: {e}"))
    }

    fn write_object(&mut self, id: usize, body: &[u8]) -> Result<(), String> {
        self.begin_object(id)?;
        self.output
            .write_all(body)
            .and_then(|_| self.output.write_all(b"endobj\n"))
            .map_err(|e| format!("PDF objectを書けません: {e}"))
    }

    fn finish(mut self) -> Result<W, String> {
        let xref_offset = self
            .output
            .stream_position()
            .map_err(|e| format!("xref offsetを取得できません: {e}"))?;
        write!(self.output, "xref\n0 {}\n", self.offsets.len())
            .map_err(|e| format!("xrefを書けません: {e}"))?;
        self.output
            .write_all(b"0000000000 65535 f \n")
            .map_err(|e| format!("xref free entryを書けません: {e}"))?;
        for offset in self.offsets.iter().skip(1) {
            writeln!(self.output, "{offset:010} 00000 n ")
                .map_err(|e| format!("xref entryを書けません: {e}"))?;
        }
        write!(
            self.output,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
            self.offsets.len(),
            xref_offset
        )
        .map_err(|e| format!("PDF trailerを書けません: {e}"))?;
        Ok(self.output)
    }
}

fn jpeg_components(jpeg: &[u8]) -> Result<u8, String> {
    use zune_jpeg::JpegDecoder;
    use zune_jpeg::zune_core::bytestream::ZCursor;

    let mut decoder = JpegDecoder::new(ZCursor::new(jpeg));
    decoder
        .decode_headers()
        .map_err(|error| format!("JPEG headerを解析できません: {error:?}"))?;
    decoder
        .info()
        .map(|info| info.components)
        .ok_or_else(|| "JPEG headerに画像情報がありません".to_string())
}

fn calculate_centered_placement(page: &PageRecord) -> Result<Placement, String> {
    if page.crop_width == 0 || page.crop_height == 0 {
        return Err(format!("{}のcrop寸法が0です", page.stem));
    }
    let scale = (A4_WIDTH_PT / page.crop_width as f32).min(A4_HEIGHT_PT / page.crop_height as f32);
    let width = page.crop_width as f32 * scale;
    let height = page.crop_height as f32 * scale;
    Ok(Placement {
        x: (A4_WIDTH_PT - width) * 0.5,
        y: (A4_HEIGHT_PT - height) * 0.5,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book_scan::BookScanStage;
    use std::path::PathBuf;

    fn record() -> PageRecord {
        PageRecord {
            index: 0,
            page_number: 10,
            stem: "p0010".to_string(),
            source_path: PathBuf::new(),
            source_width: 1654,
            source_height: 2339,
            is_blank: false,
            blank_metrics: None,
            crop_path: None,
            crop_width: 1200,
            crop_height: 1900,
            restore_x: 200.0,
            restore_y: 170.0,
            processed_path: None,
            jpeg_path: None,
            stage: BookScanStage::ScanTailored,
            attempts: 0,
            error: None,
        }
    }

    #[test]
    fn placement_preserves_source_coordinates() {
        let placement = calculate_placement(&record()).unwrap();
        assert!((placement.x - 71.98).abs() < 0.1);
        assert!((placement.width - 431.9).abs() < 0.2);
        assert!(placement.y > 90.0);
    }

    #[test]
    fn blank_page_keeps_an_embedded_image() {
        let output =
            std::env::temp_dir().join(format!("img2pdf-blank-page-{}.pdf", std::process::id()));
        let jpeg =
            std::env::temp_dir().join(format!("img2pdf-blank-page-{}.jpg", std::process::id()));
        image::RgbImage::from_pixel(8, 8, image::Rgb([255, 255, 255]))
            .save(&jpeg)
            .unwrap();
        let mut page = record();
        page.is_blank = true;
        page.crop_width = page.source_width;
        page.crop_height = page.source_height;
        page.jpeg_path = Some(jpeg.clone());
        page.stage = BookScanStage::Encoded;
        write_pdf(&[page], &output, true).unwrap();
        let bytes = std::fs::read(&output).unwrap();
        assert!(
            bytes
                .windows(b"/DCTDecode".len())
                .any(|part| part == b"/DCTDecode")
        );
        assert!(
            bytes
                .windows(b"/Count 1".len())
                .any(|part| part == b"/Count 1")
        );
        let _ = std::fs::remove_file(output);
        let _ = std::fs::remove_file(jpeg);
    }

    #[test]
    fn grayscale_jpeg_uses_device_gray() {
        let output =
            std::env::temp_dir().join(format!("img2pdf-grayscale-page-{}.pdf", std::process::id()));
        let jpeg =
            std::env::temp_dir().join(format!("img2pdf-grayscale-page-{}.jpg", std::process::id()));
        image::GrayImage::from_pixel(8, 8, image::Luma([180]))
            .save(&jpeg)
            .unwrap();
        let mut page = record();
        page.jpeg_path = Some(jpeg.clone());
        write_pdf(&[page], &output, true).unwrap();
        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.windows(11).any(|part| part == b"/DeviceGray"));
        let _ = std::fs::remove_file(output);
        let _ = std::fs::remove_file(jpeg);
    }
}
