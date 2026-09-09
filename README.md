# img2pdf

JPEG画像群をPDFへまとめるRust製アプリです。PDFからの画像抽出・レンダリングと、写真・スキャン由来の本を読みやすく整える`book-scan`モードも提供します。CLIとGUIの両方から利用できます。

## インストール

Rust toolchainを用意し、リポジトリ直下で実行します。

```bash
cargo install --path .
```

GUIのビルドにはFLTKが使用されます。PDF入力や本モードでは、用途に応じて次の外部ツールも必要です。

- Poppler: `pdfimages`、`pdfinfo`、`pdftoppm`
- ScanTailor Advanced: `scantailor-cli`
- Real-ESRGAN ncnn Vulkan: `realesrgan-ncnn-vulkan`
- JPEG無劣化最適化: `jpegtran`

## 基本的な使い方

JPEGフォルダをPDFへ変換します。

```bash
img2pdf input-images output.pdf
```

埋め込みJPEGを可能な限り無劣化で再利用する場合は`--lossless`を指定します。

```bash
img2pdf input-images output.pdf --lossless
```

PDFから画像を抽出またはレンダリングします。`auto`では埋め込みJPEGを無劣化抽出できないページだけPNGへレンダリングします。

```bash
img2pdf pdf2img input.pdf output-images --format auto
```

引数なしで起動するとGUIが開きます。各モードの全オプションは次のコマンドで確認できます。

```bash
img2pdf --help
img2pdf pdf2img --help
img2pdf book-scan --help
```

## 本の自炊モード

`book-scan`は、PDFまたは画像フォルダへScanTailorによるページ検出・背景正規化、任意の超解像、文字調整を適用し、元画像の位置と縦横比を保ったA4 PDFを生成します。

```bash
img2pdf book-scan input.pdf output.pdf \
  --crop-exclude-pages 1 \
  --deskew off \
  --superres anime \
  --ai-scale 2 \
  --output-scale 1
```

主な既定動作は次のとおりです。

- 表紙の1ページ目を自動cropから除外
- 背景正規化とカラー紙面補正を有効化
- 傾き補正、湾曲補正、全体の自動グレースケール化を無効化
- AI内部では2倍に復元し、最終画像は元の画素数相当へ戻す
- A4上で元の位置・大きさ・縦横比を保持
- 白紙ページも元画像をPDFへ埋め込み
- 処理途中の状態を検証し、ページ単位で再開

詳細な処理順、設定値、ページ指定方法は[本の自炊モードのドキュメント](docs/BOOK_SCAN_MODE.md)を参照してください。

## 外部ツールのパス

外部ツールはPATHから検索します。本モードでは明示オプションのほか、次の環境変数も利用できます。

```bash
export IMG2PDF_SCANTAILOR=/path/to/scantailor-cli
export IMG2PDF_REALESRGAN=/path/to/realesrgan-ncnn-vulkan
```

WSLからWindows版Real-ESRGANを使用する場合は`.exe`を指定できます。アプリは推論対象をWindows側の一時フォルダへ転送して処理します。

## WSLでのファイル配置

WSLから`/mnt/c`などのWindows側ファイルを大量に読み書きすると、Linux側ファイルシステムより遅くなる場合があります。本モードの中間ファイルは既定でLinuxのsystem temp配下に置かれますが、入力や最終出力の転送時間は残ります。

大きなPDFや多数の画像を処理するときは、入力を一度Linux側へコピーし、処理完了後に出力だけWindows側へ戻す運用が安定します。任意の作業場所は`book-scan --work-dir <PATH>`で指定できます。

## 開発

```bash
cargo fmt --check
cargo test --locked
```

本モードは外部プログラムを同梱せず、実行時にユーザー指定またはPATHから検出します。各外部プログラムを再配布する場合は、それぞれのライセンス条件を確認してください。
