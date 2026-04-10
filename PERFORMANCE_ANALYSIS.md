# generate pdf が遅い原因の整理

作成日: 2026-04-10
対象: Rust 実装 (`src/image_processor.rs`)

## 結論（優先度順）

1. **PDF 生成フェーズが実質シングルスレッドで重い**
2. **各ページで JPEG 再エンコードを実施している**
3. **リサイズに高コストな `Lanczos3` を使用している**
4. **全画像をメモリ保持してから PDF 化している**
5. **並列度を CPU の 70% に制限している**
6. **進捗更新で `Mutex` ロックが高頻度に発生する**

---

## 詳細分析

### 1) PDF 生成フェーズがシングルスレッド

- `run_thread` では画像前処理は `par_iter()` で並列化されているが、
  PDF 生成は `generate_pdf(&success_images, ...)` を 1 回呼ぶ構成。
- `generate_pdf` 内は `for (page_idx, img) in images.iter().enumerate()` で順次処理。
- 最後に `doc.save(...)` で全体を書き出すため、保存フェーズは重くなりやすい。

根拠:
- `par_iter()` 使用: `src/image_processor.rs:128`
- `generate_pdf(...)` 呼び出し: `src/image_processor.rs:187`
- 逐次ループ: `src/image_processor.rs:248`
- 最終保存: `src/image_processor.rs:294`

影響:
- 画像枚数が増えるほど保存待ち時間が直線的に増える。

### 2) 各ページで JPEG 再エンコード

- `generate_pdf` の各ページで `encode_jpeg(img)?` を呼び、
  `JpegEncoder::new_with_quality(..., PDF_QUALITY)` で再圧縮している。
- 画像ごとに CPU を使う処理が追加される。

根拠:
- `encode_jpeg` 実装: `src/image_processor.rs:212`
- JPEG 品質指定: `src/image_processor.rs:215`
- PDF 生成中の再エンコード: `src/image_processor.rs:263`
- 品質設定値: `src/utils/constants.rs:20` (`PDF_QUALITY = 80`)

影響:
- 高解像度画像・ページ数増加で CPU 時間が大きく増える。

### 3) リサイズに `Lanczos3` を使用

- `process_single_image` で `image::imageops::FilterType::Lanczos3` を使用。
- Lanczos は高品質だが計算コストが高い。

根拠:
- リサイズフィルタ: `src/image_processor.rs:78`

影響:
- 画像処理フェーズが重くなり、特に大きな元画像で顕著。

### 4) 全画像をメモリ保持してから PDF 化

- 前処理後、`success_images: Vec<RgbImage>` に全件蓄積。
- その後にまとめて PDF 生成するため、メモリ圧迫とキャッシュ効率低下が起こりうる。

根拠:
- `success_images` への蓄積: `src/image_processor.rs:157-165`
- まとめて `generate_pdf` 呼び出し: `src/image_processor.rs:187`

影響:
- 枚数が多いとスワップやメモリ帯域飽和で体感速度が低下。

補足（目安）:
- デフォルト幅 1654px の A4 比率キャンバスは約 `1654x2339`。
- 生 RGB は 1 枚あたり約 `1654 * 2339 * 3 ≒ 11.6MB`。
- 100 枚なら単純計算で約 `1.16GB`（実際は追加オーバーヘッドあり）。

根拠:
- デフォルト幅: `src/utils/constants.rs:11`
- A4 比率: `src/utils/constants.rs:14`

### 5) 並列度を CPU の 70% に制限

- スレッド数を `available_parallelism * 0.7` で設定している。
- PC の状態によっては処理時間短縮余地を残す設定。

根拠:
- スレッド数計算: `src/image_processor.rs:13-18`
- グローバルプール設定: `src/image_processor.rs:25-30`

影響:
- 最大性能より安全側（他アプリとの共存重視）。

### 6) 進捗更新で `Mutex` ロック

- 各画像処理完了ごとに `Mutex` をロックしてカウンタ更新。
- 画像数が多い場合、ロック競合が微小ながら積み上がる可能性。

根拠:
- カウンタ初期化: `src/image_processor.rs:124`
- ロックして更新: `src/image_processor.rs:138-145`

影響:
- 主因ではないが、スループット低下要因になりうる。

---

## 補足: 今回は主にコード構造からの原因分析

この文書は、現状コードの処理構造と設定値から整理したボトルネック分析。
厳密な寄与率（例: 何%が JPEG エンコード時間か）を出すには、
処理フェーズごとの計測ログ（Processing/Saving の実測秒）を追加して確認するのが望ましい。

---

## Agent向け実行ドキュメント（品質維持・CPU使用率据え置き）

### 目的

- 出力品質を維持したまま、`generate pdf` の体感時間を短縮する。
- CPU使用率の方針は現状維持（現行の約70%運用を変更しない）。

### 固定制約（必須）

- `PDF_QUALITY` は変更しない（現状値を維持）。
- `--width` のデフォルト値と解像度方針は変更しない。
- `calc_worker_threads()` の 70% 方針は変更しない。
- 画質劣化を伴うフィルタ変更（例: `Lanczos3` から他フィルタへ変更）は行わない。

### 非目標（今回やらないこと）

- CPU使用率を上げる最適化。
- 画質や出力解像度を下げる最適化。
- UI/CLI仕様の大きな変更。

### 実装タスク

1. **進捗カウンタのロック削減**
  - `Mutex<usize>` を `AtomicUsize` に置換し、進捗更新のロック競合を減らす。
  - 期待効果: 並列処理時のオーバーヘッド微減。

2. **JPEGエンコードの並列先行化**
  - 画像前処理後に、`RgbImage -> JPEG bytes` を並列で作成。
  - PDFドキュメントへの追加は順序保持で逐次実行する。
  - 期待効果: 重いエンコード処理をCPU上限内で効率化。

3. **全件生画像保持の縮小（段階的）**
  - `Vec<RgbImage>` の長期保持を避ける方向へ寄せる。
  - 最低限、PDF追加済みページの不要データを速やかに解放できる構造にする。
  - 期待効果: メモリ圧迫軽減による後半の失速抑制。

4. **コピー回数の削減**
  - 中間 `Vec` の再配置や不要コピーを見直す。
  - 並び順保証は維持する。
  - 期待効果: 小さな改善の積み上げ。

### 受け入れ基準

- 品質同等性:
  - 同一入力・同一設定で、目視差分なし。
  - 出力 PDF サイズの極端な増減がない（品質変更を疑う差がない）。
- 性能:
  - 同一入力セットで総時間が短縮、または少なくとも悪化しない。
  - `Saving` フェーズ時間の短縮が確認できる。
- CPU方針:
  - 実行中CPU使用率の運用方針が現状同等（70%運用）であること。

### 計測プロトコル

- 比較は同一マシン・同一入力セットで3回実施し、中央値を採用。
- 固定条件:
  - `--release`
  - 同じ `--width`
  - 同じ `PDF_QUALITY`
  - 同じファイル順
- 記録項目:
  - 総時間
  - Processing 時間
  - Saving 時間
  - ピークメモリ使用量
  - 出力PDFサイズ

### 対象コード

- `src/image_processor.rs`（主対象）
- `src/utils/constants.rs`（値は参照のみ、変更しない）
- 必要に応じて `tests/image_processor_tests.rs`（回帰テスト追加）

### 参照根拠（現状）

- 並列処理の起点: `src/image_processor.rs:128`
- 進捗カウンタのロック: `src/image_processor.rs:124`, `src/image_processor.rs:138-145`
- PDF生成呼び出し: `src/image_processor.rs:187`
- PDF内逐次処理: `src/image_processor.rs:248`
- JPEG再エンコード: `src/image_processor.rs:263`
- 最終保存: `src/image_processor.rs:294`
