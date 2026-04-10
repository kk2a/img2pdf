# Rust 移行 - 引き継ぎ資料一覧

このフォルダには、Python で書かれた「画像結合PDF生成ツール」をRustに移行するための引き継ぎ資料が含まれています。

---

## 📋 ドキュメント一覧

### 1. **RUST_MIGRATION_GUIDE.md** (メインドキュメント)
   
   最初に読むべき総合ガイド。以下を網羅：
   - プロジェクト概要とアーキテクチャ
   - モジュール構成と責務
   - データ構造と処理フロー
   - 定数とパラメータ
   - 外部依存ライブラリとRust対応
   - エラーハンドリング戦略
   - パフォーマンス要件
   - テスト項目チェックリスト
   - **推奨読了時間: 1-2時間**

### 2. **RUST_PROJECT_TEMPLATE.md** (実装用テンプレート)

   実装時に参考する構体とサンプルコード：
   - `Cargo.toml` テンプレート（FLTK/Iced/GTK版）
   - プロジェクト構成案
   - `src/main.rs` サンプル構造
   - モジュール設計サンプル
   - `ImageProcessor` スケルトン
   - `AppConfig` 実装例
   - **推奨読了時間: 30-45分**

### 3. **PYTHON_TO_RUST_COMPARISON.md** (コード対比ガイド)

   Python コードの各部分がRustでどう書くかを示す：
   - インポートと定数の対応
   - クラスメソッドの Rust 実装
   - 画像処理の実装比較
   - 並列処理の書き方
   - エラーハンドリング比較
   - メモリ管理の違い
   - テスト記述の比較
   - **推奨読了時間: 45分～1時間**

---

## 🚀 移行ロードマップ

### フェーズ 1: 基礎設定（1-2日）
```
① プロジェクト初期化
   - cargo new image_to_pdf
   - Cargo.toml 設定（RUST_PROJECT_TEMPLATE.md 参照）

② 定数・ユーティリティの実装
   - src/utils/constants.rs
   - src/utils/paths.rs

③ データ型定義
   - src/models/config.rs
   - src/models/error.rs
```

### フェーズ 2: コア画像処理（2-3日）
```
① AppConfig の実装
   - 設定ファイル読み書き
   - クロスプラットフォーム対応

② ImageProcessor - 単一画像処理
   - JPEG読み込み
   - EXIF回転処理
   - リサイズとキャンバス配置

③ テスト作成
   - 単体テスト
   - 画像処理ロジックの検証
```

### フェーズ 3: 並列・PDF処理（2-3日）
```
① 並列処理統合
   - rayon による並列化
   - 進捗通知メカニズム

② PDF生成
   - printpdf または pdfium-render
   - 複数ページ出力

③ エラーハンドリング
   - 回復可能・致命的エラー分離
   - ユーザー通知メカニズム
```

### フェーズ 4: GUI実装（3-4日）
```
① GUI フレームワーク選定
   - FLTK-rs / Iced / GTK-rs から選択

② UI 構築
   - ファイル選択ダイアログ
   - 設定画面
   - 進捗表示

③ バックエンド統合
   - イベントハンドラ実装
   - スレッド間通信
```

### フェーズ 5: テスト・最適化（2-3日）
```
① 統合テスト
   - エンドツーエンドテスト
   - エラーハンドリング検証

② パフォーマンス測定
   - ベンチマーク実施
   - メモリ使用量確認

③ クロスプラットフォーム テスト
   - Windows / macOS / Linux
```

**総所要時間: 2-3週間** (1人で専任の場合)

---

## 🛠️ 初期セットアップ

### Step 1: Rust インストール
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Step 2: 新規プロジェクト作成
```bash
cargo new image_to_pdf
cd image_to_pdf
```

### Step 3: Cargo.toml を編集
RUST_PROJECT_TEMPLATE.md の Cargo.toml サンプルを参考に設定

### Step 4: 初回ビルド
```bash
cargo build
cargo test
```

---

## 📊 主要な変更点

| 項目 | Python | Rust | 影響度 |
|-----|--------|------|-------|
| **エラー処理** | try/except | Result<T, E> | 高 |
| **並列処理** | ThreadPoolExecutor | rayon | 中 |
| **メモリ管理** | GC 自動 | 所有権管理 | 中 |
| **型システム** | 動的型 | 静的型 | 高 |
| **GUI** | tkinter | FLTK/Iced/GTK | 中 |
| **開発速度** | 高速 | 低速（学習コスト有） | 中 |
| **実行速度** | 標準 | 2-3倍高速 | 低 |

---

## 🧪 テスト戦略

### 単体テスト
```rust
#[test]
fn test_calculate_height() { }

#[test]
fn test_process_single_image() { }

#[test]
fn test_load_config() { }
```

### 統合テスト
```bash
tests/integration_tests.rs
- フル処理フロー
- エラーケース
```

### パフォーマンステスト
```bash
cargo bench --release
```

---

## 📦 推奨依存ライブラリ

### **必須**
- `image = "0.24"` - 画像処理 ⭐⭐⭐
- `serde_json = "1.0"` - JSON処理 ⭐⭐
- `rayon = "1.7"` - 並列処理 ⭐⭐⭐
- `anyhow = "1.0"` - エラー処理 ⭐⭐⭐

### **GUI フレームワーク（いずれか1つ）**
- `fltk-rs = "1.4"` - 軽量、推奨 ⭐⭐⭐
- `iced = "0.12"` - モダン ⭐⭐
- `gtk-rs = "0.17"` - Unix/Linux ⭐⭐

### **PDF生成（いずれか1つ）**
- `printpdf = "0.7"` - 低レベル制御 ⭐⭐⭐
- `pdfium-render = "0.8"` - 高レベルAPI ⭐⭐

### **その他**
- `dirs = "5.0"` - クロスプラットフォーム ⭐⭐⭐
- `tokio = "1.35"` - 非同期処理（オプション） ⭐
- `tracing = "0.1"` - ロギング ⭐⭐

---

## ⚠️ 注意事項

### Rust 学習曲線
- **所有権と借用**: Rust 固有の概念、学習が必要
- **コンパイルエラー**: 最初は多いが、コンパイル時に多くのバグを検出可能
- **開発時間**: Python より長いが、実行時速度と安全性がメリット

### ライブラリ制約
- **EXIF処理**: `image-exif` が小さいため、複雑な要件は要検討
- **PDF生成**: Pillow と完全互換の Rust ライブラリは少ない、`printpdf` が最有力
- **GUI**: Rust の GUI は Python ほど豊富ではない

### パフォーマンス
- **実行速度**: 2-3倍高速化が期待できる
- **メモリ**: Python より効率的だが、初期メモリフットプリントは大きい（コンパイル後のバイナリ）
- **ビルド時間**: 増加する可能性（especially リリースビルド）

---

## 📌 クイックリファレンス

### Rust での Python コード解釈

```python
# Python
try:
    result = some_operation()
except Exception as e:
    handle_error(e)
```

```rust
// Rust
match some_operation() {
    Ok(result) => use_result(result),
    Err(e) => handle_error(e),
}

// または ? 演算子
let result = some_operation()?;
```

### スレッド間通信

```python
# Python - コールバック
processor = ImageProcessor(on_progress, on_finished)
```

```rust
// Rust - チャネル
let (tx, rx) = mpsc::channel();
thread::spawn(move || {
    tx.send(Message::Progress { ... })?;
});
for msg in rx { }
```

### 並列処理

```python
# Python
with ThreadPoolExecutor(max_workers=4) as executor:
    futures = [executor.submit(task, item) for item in items]
```

```rust
// Rust
items.par_iter()
    .map(|item| task(item))
    .collect::<Vec<_>>()
```

---

## 🔗 参考リソース

### 公式ドキュメント
- [The Rust Book](https://doc.rust-lang.org/book/) - Rust の完全ガイド
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/) - 実装例
- [Cargo Book](https://doc.rust-lang.org/cargo/) - パッケージ管理

### クレート紹介
- [image](https://docs.rs/image/) - 画像処理
- [rayon](https://docs.rs/rayon/) - データ並列処理
- [fltk-rs](https://fltk-rs.github.io/fltk-book/) - GUI フレームワーク
- [serde](https://serde.rs/) - シリアライゼーション

### 参考書
- "Programming Rust, 2nd Edition" - Steve Klabnik, Carol Nichols
- "Rust for Rustaceans" - Jon Gjengset（中級向け）

---

## 👥 サポートコミュニティ

- **Rust Official Forum**: https://users.rust-lang.org/
- **Stack Overflow**: `#rust` タグ
- **Reddit**: r/rust
- **Discord**: Rust Programming Language Community

---

## 📝 ドキュメント使用ガイド

### 最初のアプローチ
```
1. RUST_MIGRATION_GUIDE.md をざっと読む（30-45分）
2. RUST_PROJECT_TEMPLATE.md で構成を理解（15-20分）
3. PYTHON_TO_RUST_COMPARISON.md で細部確認（随時参照）
```

### 実装時
```
1. 各フェーズで対応する MIGRATION_GUIDE セクションを参照
2. PYTHON_TO_RUST_COMPARISON.md でコード例を確認
3. PROJECT_TEMPLATE.md でボイラープレート をコピー
4. Rust 公式ドキュメントで詳細確認
```

### トラブル時
```
1. PYTHON_TO_RUST_COMPARISON.md の "エラーハンドリング" セクション確認
2. MIGRATION_GUIDE の "注意事項" セクション確認
3. 該当クレートの公式ドキュメント参照
```

---

## ✅ 事前準備チェッシート

- [ ] Rust インストール完了
- [ ] cargo コマンド実行確認
- [ ] IDE / エディタ設定（VS Code + rust-analyzer 推奨）
- [ ] MIGRATION_GUIDE 読了
- [ ] PROJECT_TEMPLATE 確認
- [ ] サンプルプロジェクト作成・ビルド確認
- [ ] 依存ライブラリのドキュメント確認
- [ ] チーム内で Rust の学習準備

---

## 🎯 成功のための推奨事項

1. **段階的実装**
   - 一度に全部書かない
   - フェーズごとにテスト・動作確認

2. **ドキュメント参照**
   - わからないことはすぐ調べる
   - Rust は学習曲線が急だが、投資の価値あり

3. **テスト駆動開発**
   - Rust の型システムはテストの一部
   - コンパイルを通す = 基本的な正確性の保証

4. **パフォーマンス測定**
   - 何度も最適化しない（最初は動作>速度）
   - 必要に基づいてのみ最適化

5. **コミュニティ活用**
   - 困ったら Stack Overflow や Reddit で質問
   - Rust コミュニティは親切

---

## 📈 進捗追跡

### ウィークリーマイルストーン例

**Week 1**
- [ ] 開発環境セットアップ
- [ ] モジュール設計完成
- [ ] 定数・ユーティリティ実装

**Week 2**
- [ ] AppConfig 実装完了
- [ ] 画像処理コア実装
- [ ] 単体テスト作成

**Week 3**
- [ ] 並列処理統合
- [ ] PDF生成実装
- [ ] エラーハンドリング完成

**Week 4**
- [ ] GUI フレームワーク統合
- [ ] 統合テスト実施
- [ ] パフォーマンス測定・最適化

---

**作成日:** 2026-04-10  
**Python 版詳細:** image_to_pdf.py (476行)  
**Rust 想定規模:** 2000-3000行（GUI含む）  
**チーム:** [チーム名]

---

質問や問題が発生した場合は、該当ドキュメント内のセクションを参照するか、チームリーダーに相談してください。
