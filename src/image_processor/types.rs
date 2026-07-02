/// PDF に埋め込む 1 ページ分のデータ（JPEG エンコード済み）
///
/// `RgbImage` は encode 後すぐに解放されるため、長期保持しない。
pub struct JpegPage {
    pub width: u32,
    pub height: u32,
    pub components: u8,
    pub data: Vec<u8>,
}
