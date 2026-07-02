use super::{ImageProcessor, JpegPage};

impl ImageProcessor {
    /// JPEG エンコード済みページ群から PDF ファイルを生成する
    ///
    /// JPEG バイトはすでに並列処理フェーズで生成済みであり、
    /// このフェーズでは再エンコードを行わない。
    /// pdf-writer を使用して直接 PDF バイト列を構築し書き出す。
    pub fn generate_pdf(
        pages: Vec<JpegPage>,
        output_path: &str,
        _canvas_width: u32,
    ) -> Result<(), String> {
        use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};
        use std::fs;

        if pages.is_empty() {
            return Err("No images to process".to_string());
        }

        // A4 サイズ（pt）: 1pt = 1/72 inch
        const A4_W: f32 = 595.28;
        const A4_H: f32 = 841.89;

        let mut pdf = Pdf::new();
        let total = pages.len();

        // オブジェクト ID の割り当て:
        //   1: catalog
        //   2: page tree
        //   3..3+total-1: page
        //   3+total..3+2*total-1: image XObject
        //   3+2*total..3+3*total-1: content stream
        let catalog_id = Ref::new(1);
        let page_tree_id = Ref::new(2);
        let base = 3_i32;
        let page_ids: Vec<Ref> = (0..total).map(|i| Ref::new(base + i as i32)).collect();
        let image_ids: Vec<Ref> = (0..total)
            .map(|i| Ref::new(base + total as i32 + i as i32))
            .collect();
        let content_ids: Vec<Ref> = (0..total)
            .map(|i| Ref::new(base + 2 * total as i32 + i as i32))
            .collect();

        // catalog → page tree
        pdf.catalog(catalog_id).pages(page_tree_id);

        // page tree
        pdf.pages(page_tree_id)
            .kids(page_ids.iter().copied())
            .count(total as i32);

        // 各ページを書き出す
        let image_name = Name(b"Im0");
        for (i, page) in pages.into_iter().enumerate() {
            // ページ定義
            let mut pdf_page = pdf.page(page_ids[i]);
            pdf_page.media_box(Rect::new(0.0, 0.0, A4_W, A4_H));
            pdf_page.parent(page_tree_id);
            pdf_page.contents(content_ids[i]);
            pdf_page
                .resources()
                .x_objects()
                .pair(image_name, image_ids[i]);
            pdf_page.finish();

            // 画像 XObject（JPEG バイトをそのまま DCTDecode で埋め込む）
            let mut img = pdf.image_xobject(image_ids[i], &page.data);
            img.filter(Filter::DctDecode);
            img.width(page.width as i32);
            img.height(page.height as i32);
            match page.components {
                1 => img.color_space().device_gray(),
                4 => img.color_space().device_cmyk(),
                _ => img.color_space().device_rgb(),
            }
            img.bits_per_component(8);
            img.finish();

            // コンテンツストリーム: 画像をアスペクト比維持で中央配置
            // PDF 座標系は左下原点。XObject は 1×1 なので行列で拡大・平行移動する。
            let sx = A4_W / page.width as f32;
            let sy = A4_H / page.height as f32;
            let scale = sx.min(sy);
            let draw_w = page.width as f32 * scale;
            let draw_h = page.height as f32 * scale;
            let offset_x = (A4_W - draw_w) * 0.5;
            let offset_y = (A4_H - draw_h) * 0.5;

            let mut content = Content::new();
            content.save_state();
            content.transform([draw_w, 0.0, 0.0, draw_h, offset_x, offset_y]);
            content.x_object(image_name);
            content.restore_state();
            pdf.stream(content_ids[i], &content.finish());
        }

        let bytes = pdf.finish();
        fs::write(output_path, &bytes).map_err(|e| e.to_string())?;

        Ok(())
    }
}
