# Rust移行ガイド - 画像結合PDF生成ツール

## 1. プロジェクト概要

**アプリケーション名:** ImageToPdf  
**言語:** Python → Rust への移行  
**主機能:** 複数のJPEG画像をA4比率のキャンバスに配置して、単一のPDFに結合  

### 核心機能
- JPEG画像のキャンバスへ中央配置 (アスペクト比保持)
- EXIF回転情報の自動処理
- 並列画像処理（CPU数の半分のスレッド）
- PDF出力
- GUI操作とリアルタイム進捗表示

---

## 2. アーキテクチャ概要

```
┌─────────────────────────────────────────────┐
│         GUI Layer (tkinter → ?)             │
│  ユーザー入力, ファイル選択, 進捗表示       │
└────────────────────┬────────────────────────┘
                     │
┌────────────────────▼────────────────────────┐
│      Application Layer (MainApp)            │
│  UIイベント処理, 状態管理, コールバック     │
└────────────────────┬────────────────────────┘
                     │
┌────────────────────▼────────────────────────┐
│     Business Logic (ImageProcessor)         │
│  画像処理, PDF生成, 並列処理 (ThreadPool)   │
└────────────────────┬────────────────────────┘
                     │
┌────────────────────▼────────────────────────┐
│     Data Access Layer (AppConfig)           │
│  設定ファイル読み書き (JSON)                │
└─────────────────────────────────────────────┘
```

---

## 3. モジュール構成と責務

### 3.1 `AppConfig` クラス
**責務:** ユーザー設定の永続化

| メソッド | 入力 | 出力 | 説明 |
|---------|------|------|------|
| `__init__()` | なし | 自身 | プラットフォーム別設定ディレクトリを初期化 |
| `_get_config_dir()` | なし | Path | 設定ディレクトリパスを取得/作成 |
| `load()` | なし | なし | `config.json` をメモリに読み込み |
| `save()` | なし | なし | メモリの設定を `config.json` に保存 |

**設定項目:**
```json
{
  "last_save_dir": "/path/to/last/save/directory"
}
```

**プラットフォーム別パス:**
- Windows: `%APPDATA%/ImageToPdf/config.json`
- macOS: `~/Library/Application Support/ImageToPdf/config.json`
- Linux: `~/.config/ImageToPdf/config.json`

---

### 3.2 `ImageProcessor` クラス
**責務:** 画像処理とPDF生成の中核ロジック

#### 処理フロー

```
入力ファイルリスト
    ↓
┌─ ThreadPoolExecutor 開始 ─────────┐
│  max_workers = CPU数 / 2 または 4  │
│                                    │
│  各ファイルに対して並列実行:       │
│  ├─ JPEG読み込み                   │
│  ├─ EXIF回転を適用                 │
│  ├─ RGB色空間に変換                │
│  ├─ アスペクト比保持でリサイズ     │
│  ├─ A4キャンバス上に中央配置       │
│  └─ 完了コールバック送信           │
└────────────────────────────────────┘
    ↓
成功画像リストを集計
    ↓
PDF生成 (Pillow)
    ↓
完了コールバック送信
```

#### メソッド詳細

| メソッド | 入力パラメータ | 戻値 | 説明 |
|---------|--------------|------|------|
| `calculate_height(width)` | width: int | int | A4比率からキャンバス高さを計算 |
| `process_single_image(file_path, canvas_w, canvas_h)` | file_path: str, canvas_w: int, canvas_h: int | (PIL.Image \| None, error tuple \| None) | 単一画像を処理 |
| `run(file_list, canvas_width, output_path)` | 各パラメータ | なし | 非同期処理を開始（別スレッド） |
| `_run_thread(...)` | 同上 | なし | スレッド内で実行されるメイン処理 |

#### `process_single_image` の詳細処理

```
1. JPEG読み込み
   ├─ PIL.Image.open() で読み込み
   └─ FileNotFoundError, PIL.UnidentifiedImageError をキャッチ

2. EXIF回転を適用
   └─ ImageOps.exif_transpose() 使用（エラー時は無視）

3. RGB色空間に変換
   ├─ CMYK または RGBA の場合のみ conversion
   └─ RGB の場合はスキップ

4. アスペクト比保持でリサイズ
   ├─ スケール計算: scale = min(canvas_w / orig_w, canvas_h / orig_h)
   ├─ 新サイズ: new_w = round(orig_w * scale)
   ├─ 新サイズ: new_h = round(orig_h * scale)
   └─ img.resize((new_w, new_h)) 実行

5. キャンバスに中央配置
   ├─ 白色 (255,255,255) のキャンバス作成
   ├─ オフセット計算: offset_x = (canvas_w - new_w) // 2
   ├─ オフセット計算: offset_y = (canvas_h - new_h) // 2
   └─ img.paste() で中央に配置

6. 戻値
   └─ (成功画像, None) または (None, エラー情報)
```

#### `/run_thread` の詳細処理

```
1. キャンバス高さ計算
   └─ canvas_height = calculate_height(canvas_width)

2. ThreadPoolExecutor 開始
   ├─ max_workers = cpu_count() // 2 or 4
   └─ future → index のマッピング作成

3. as_completed() でタスク完了を追跡
   ├─ 完了時にコールバック送信
   └─ インデックスごとに結果を保存（順序保持）

4. 完了後、成功/失敗を分類
   ├─ success_images: 成功した画像リスト
   └─ errors: エラー情報リスト

5. PDF生成 (Pillow)
   ├─ first_image.save() で複数ページPDFを生成
   ├─ resolution: 72.0 DPI
   ├─ quality: 80
   └─ エラー時は final_errors に追加

6. 最終コールバック送信
   └─ (成功/失敗, 成功数, 失敗数, エラーリスト, 出力パス)
```

---

### 3.3 `MainApp` クラス
**責務:** GUI制御と全体の状態管理

#### UI構成

```
┌─ 1. 入力 ──────────────────────────┐
│ [フォルダを選択] [ファイルを追加]   │
│ 対象ファイル数: 0枚               │
├─ 2. 設定 ──────────────────────────┤
│ キャンバス幅 (px): [1654]        │
│ キャンバス高さ: 2336 px          │
├─ 3. 出力 ──────────────────────────┤
│ [保存先を選択] [未選択.........]   │
│ [PDF生成を実行]                    │
├─ 4. 進捗 ──────────────────────────┤
│ [████████░░░░░░░░░░░░] 50%       │
│ 画像処理中 (5/10)                 │
└────────────────────────────────────┘
```

#### メソッド一覧

| メソッド | 役割 |
|---------|------|
| `__init__()` | ウィンドウと初期状態を初期化 |
| `setup_ui()` | UIパーツを構築 |
| `select_folder()` | フォルダ選択ダイアログを開く |
| `add_files()` | ファイル選択ダイアログを開く |
| `update_input_status()` | ステータスラベルを更新 |
| `select_output()` | 保存先選択ダイアログを開く |
| `update_ui_state()` | ボタンの有効/無効を切り替え |
| `on_width_change()` | キャンバス幅変更時にリスナが呼ぶ |
| `run_processing()` | PDF生成処理を開始 |
| `update_progress()` | ImageProcessor からの進捗コールバック |
| `_update_progress_ui()` | UI更新（スレッドセーフ） |
| `on_finished()` | ImageProcessor からの完了コールバック |
| `_on_finished_ui()` | 完了処理（スレッドセーフ） |

---

## 4. データ構造

### 4.1 ファイルリスト
```python
file_list: List[str]
# 例: ['/path/to/image1.jpg', '/path/to/image2.jpeg', ...]
# ソート順: ファイル名の辞書順（小文字）
```

### 4.2 処理結果
```python
processed_results: List[(PIL.Image | None, (str, str) | None)]
# [(画像1, None), (None, ("file2.jpg", "Permission denied")), ...]
# インデックス順序は入力ファイルと同じ
```

### 4.3 エラー情報
```python
errors: List[(str, str)]
# [("file1.jpg", "IOError: ..."), ("file2.jpg", "PIL error: ..."), ...]
```

---

## 5. 定数とパラメータ

| 定数名 | 値 | 説明 |
|-------|-----|------|
| `APP_NAME` | "ImageToPdf" | アプリケーション名 |
| `CONFIG_DIR_NAME` | "ImageToPdf" | 設定ディレクトリ名 |
| `CONFIG_FILE_NAME` | "config.json" | 設定ファイル名 |
| `DEFAULT_WIDTH` | 1654 | デフォルトキャンバス幅（ピクセル） |
| `A4_RATIO` | 1.41421356 | A4用紙 高さ/幅 比率 |
| `PDF_RESOLUTION` | 72.0 | PDF DPI |
| `PDF_QUALITY` | 80 | PDF品質（1-95） |
| `CANVAS_COLOR` | (255, 255, 255) | キャンバス背景色（RGB） |

### 計算式

```
キャンバス高さ = キャンバス幅 * A4_RATIO
             = キャンバス幅 * 1.41421356

リサイズスケール = min(canvas_w / orig_w, canvas_h / orig_h)

新幅 = round(元幅 * スケール)
新高さ = round(元高さ * スケール)

X オフセット = (canvas_w - 新幅) // 2
Y オフセット = (canvas_h - 新高さ) // 2
```

---

## 6. ファイル入出力

### 6.1 入力
- **ファイル形式:** JPEG (.jpg, .jpeg)
- **対応フォーマット:** jpeg, jpg
- **エンコーディング:** 自動判定（EXIF含む）
- **フォルダ参照:** 再帰的に探索しない（ルートフォルダのみ）
- **複数選択:** サポート

### 6.2 出力
- **ファイル形式:** PDF
- **DPI:** 72
- **品質:** 80
- **デフォルト名:** "output.pdf"

---

## 7. エラーハンドリング戦略

### 7.1 回復可能なエラー

| エラー | 処理 | ユーザー通知 |
|-------|------|------------|
| ファイル読み込み失敗 (IOError) | スキップ、次ファイルへ | エラー詳細表示 |
| EXIF回転適用失敗 | 無視、進行 | 表示しない |
| 画像形式異常 | スキップ | エラー詳細表示 |
| 権限不足 | スキップ | エラー詳細表示 |

### 7.2 致命的エラー

| エラー | 処理 | ユーザー通知 |
|-------|------|------------|
| PDF生成失敗 | 処理中止 | エラーダイアログ |
| すべての画像処理失敗 | PDF生成しない | エラーダイアログ |
| 設定保存失敗 | サイレント | なし |
| パス作成失敗 | サイレント | なし |

---

## 8. 並列処理の詳細

### 8.1 スレッド設定

```python
max_workers = os.cpu_count() / 2 or 4
# CPU数が4の場合: 2ワーカー
# CPU数が8の場合: 4ワーカー
# CPU数なしの場合: 4ワーカー
```

### 8.2 処理順序

```
入力: [a.jpg, b.jpg, c.jpg, d.jpg]

─ Thread 1 ──→ a.jpg 処理 (時間: 100ms)
─ Thread 2 ──→ b.jpg 処理 (時間: 120ms) ← 完了 (120ms)
    ↓ 再利用
    c.jpg 処理 (時間: 110ms)   ← 完了 (230ms)
         ↓ 再利用
         (次のタスクなし)

─ Thread 1 ──→ b.jpg 処理 (時間: 120ms) ← 完了 (220ms)
    ↓ 再利用
    d.jpg 処理 (時間: 130ms)   ← 完了 (350ms)

結果保持: [a画像, b画像, c画像, d画像] ← インデックスで順序保持
```

### 8.3 進捗表示

```
completed_count: 0 → 1 → 2 → 3 → 4
callback 送信時機: タスク完了時 (完了順序)
進捗バー計算: (completed_count / total) * 90%
```

---

## 9. 外部依存とRust対応

### 9.1 Python依存

| ライブラリ | 用途 | Rust 対応 |
|-----------|------|----------|
| tkinter | GUI | `fltk-rs`, `gtk-rs`, `iced`, `druid` など |
| PIL/Pillow | 画像処理 | `image`, `img-parts` |
| json | 設定保存 | `serde_json` |
| threading | 並列処理 | `std::thread`, `rayon` |
| concurrent.futures | ThreadPool | `rayon`, `tokio` |
| pathlib | パス操作 | `std::path::Path` |
| os | システム情報 | `std::env` |

### 9.2 推奨Rustクレート

```toml
[dependencies]
# 画像処理
image = "0.24"
imageproc = "0.23"  # 追加フィルタ

# PDF生成
printpdf = "0.7"  # or pdfium-render
# または Pillow 互換性のため Python との連携

# GUI
fltk-rs = "1.4"  # または iced, gtk-rs

# 並列処理
rayon = "1.7"
tokio = "1.35"  # 非同期用

# JSON
serde_json = "1.0"
serde = { version = "1.0", features = ["derive"] }

# パス操作とファイルシステム
std = (標準ライブラリ)

# ロギング
tracing = "0.1"
tracing-subscriber = "0.3"
```

---

## 10. 標準入出力フロー

### 10.1 典型的なユーザーワークフロー

```
1. アプリ起動
   ↓
2. フォルダ/ファイル選択
   ├─ フォルダ選択 → JPEG検索→ ソート → file_list に格納
   └─ ファイル選択 → ソート → file_list に格納
   ↓
3. キャンバス幅を確認/変更
   ↓
4. 出力パスを選択
   ↓
5. "PDF生成を実行" ボタンクリック
   ├─ 入力検証 (幅 > 0)
   ├─ 設定保存
   └─ ImageProcessor.run() 呼び出し
   ↓
6. 進捗表示
   ├─ 進捗バー 0〜90%
   └─ "画像処理中 (x/y)" 表示
   ↓
7. PDF生成中
   ├─ 進捗バー 95%
   └─ "PDF書き出し中..." 表示
   ↓
8. 完了コールバック
   ├─ 進捗バー 100%
   ├─ ステータス更新
   └─ メッセージボックス表示
   ↓
9. UI再ロック解除
```

---

## 11. パフォーマンス要件

### 11.1 ベンチマーク (参考値)

| 処理 | 仕様 | 目安時間 |
|------|------|---------|
| JPEG読み込み | 2000x1500, 300KB | 10-20ms |
| EXIF処理 | 回転含む | 2-5ms |
| リサイズ | 2000x1500 → 800x1100 | 30-50ms |
| キャンバス配置 | paste操作 | 5-10ms |
| 単一画像処理 | 全工程 | 50-100ms |
| **100枚処理** | max_workers=4 | 1500-2000ms (1.5-2s) |
| PDF生成 | 100ページ | 500-1000ms |
| **全処理** | 100枚 | 2000-3000ms (2-3s) |

### 11.2 最適化ポイント

- **GIL回避:** Python の GIL を避けるため画像処理は ThreadPoolExecutor 使用
- **Rust移行時:** rayon の par_iter() で CPU バウンドの並列化
- **メモリ:** 大量画像処理時メモリ効率に注意
- **PDF生成:** ストリーミング出力による大規模ファイル対応

---

## 12. 注意事項と実装上の留意点

### 12.1 Rust移行時の重要事項

1. **スレッドセーフ:**
   - Rust のスレッド安全性により自動的に安全
   - 共有メモリアクセスは不可（所有権により保証）

2. **メモリ管理:**
   - 生画像メモリが大きい (2000x1500 RGB ≈ 9MB)
   - 100枚なら約 900MB (ピーク時)

3. **GUI フレームワーク選択:**
   - `fltk-rs`: 軽量、クロスプラットフォーム
   - `iced`: モダン（Elm風）、学習コスト有
   - `gtk-rs`: Unix/Linux向け、Windows対応可

4. **PDF生成ライブラリ:**
   - `printpdf`: 低レベル、細かい制御可
   - `pdfium-render`: 高レベル、安定性重視
   - **選択:** Pillow の `save(..., "PDF")` と同等の機能を探す必要有

5. **EXIF 処理:**
   - `image-exif` クレート使用推奨
   - または `piexif` (Python) との連携

6. **エラー処理:**
   - Rust の Result/Option 型で明示的に
   - Python の try/except に比べ、より厳密

7. **設定ファイル:**
   - `serde_json` で十分
   - または `toml`, `ron` も検討

### 12.2 Python コードの微妙な動作

```python
# 1. ThreadPoolExecutor は実行順序保証なし
#    but as_completed() で進捗追跡、インデックスで画像順序保持

# 2. エラーのサイレント無視
#    EXIF処理失敗、設定保存失敗は黙って続行

# 3. オフセット計算は整数除算
#    offset_x = (canvas_w - new_w) // 2  ← 端数は下方丸め

# 4. ファイル形式は小文字マッチ
#    f.suffix.lower() in ['.jpg', '.jpeg']

# 5. コールバック非同期実行
#    GUI更新は self.after() で メインスレッド移譲
```

---

## 13. テスト項目チェックリスト

- [ ] 単一JPEG読み込みと処理
- [ ] 複数JPEG処理（順序保持確認）
- [ ] EXIF回転情報の適用
- [ ] 色空間変換（CMYK, RGBA → RGB）
- [ ] アスペクト比保持のリサイズ
- [ ] キャンバス中央配置の計算
- [ ] PDF生成（単一ページ、複数ページ）
- [ ] エラーハンドリング（ファイル不在、破損画像など）
- [ ] 並列処理のスレッド数制御
- [ ] GUI ウィンドウ表示と操作
- [ ] 設定ファイル保存/読み込み（プラットフォーム別）
- [ ] 大量画像処理時のメモリ使用量
- [ ] キャンセル機能（実装する場合）

---

## 14. 推奨実装順序

1. **コア画像処理モジュール**
   - 単一画像読み込み → リサイズ → キャンバス配置

2. **Config/IO レイヤー**
   - JSON読み書き、ファイルシステム操作

3. **ImageProcessor (シングルスレッド版)**
   - 複数画像を順次処理

4. **並列処理統合**
   - rayon/tokio でマルチスレッド化

5. **PDF生成**
   - 適切なクレート選定・統合

6. **GUI フレームワーク**
   - FLTK / Iced など選定、統合

7. **統合テストと最適化**

---

## 15. 参考リソース

- **Pillow 公式ドキュメント:** https://pillow.readthedocs.io/
- **Rust Image クレート:** https://github.com/image-rs/image
- **Rayon (並列処理):** https://docs.rs/rayon/
- **Serde (JSON):** https://serde.rs/
- **FLTK-rs GUI:** https://github.com/fltk-rs/fltk-rs
