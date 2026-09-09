# 本の自炊モード

`book-scan` は、写真・スキャン由来のPDFまたは画像フォルダを前処理し、元の紙面位置を保ったA4 PDFを生成するモードです。

## 既定パイプライン

1. PDF内のJPEGを可能なら無劣化抽出し、不可能な場合はPopplerでレンダリング
2. 保守的に空白ページを検出
3. ScanTailor Advancedでページ領域検出と背景照明正規化（既定で表紙の1ページ目はcrop除外）
4. 明るく低彩度な紙面だけをRGBカラー背景補正
5. 必要な場合だけ、黒文字の核から2px以内の色差を弱める（既定OFF）
6. `realesr-animevideov3` で内部x2推論し、元解像度相当へ縮小
7. 3x3 Minimumを15%混ぜて文字線を補強
8. RGB JPEG quality 90、4:4:4で保存
9. crop前の座標へ戻し、白いA4画像を作らずPDFへ直接配置

傾き補正、湾曲補正、黒インク色差補正、グレースケール、spriteは既定では使用しません。

傾き補正をONにした場合、ScanTailor Advanced 1.0.16のCLIでは`--deskew=auto`を明示すると内部的にmanual 0°として保存されるため、アプリはdeskew/rotate指定を省略してScanTailorの既定autoを使用します。OFFの場合だけ`--rotate=0 --deskew=manual`を明示します。明解演習線形代数p.80–83ではp.81とp.82で残留傾きが改善し、元位置保持でのA4上の移動は最大約1.5mmでした。p.80は0°判定のままです。

## 採用状態

| 機能 | 既定 | 実行時設定 |
|---|---:|---|
| 自動crop（表紙p.1を除外） | ON | ON/OFF、除外ページ、余白、検出tolerance |
| ScanTailor背景正規化・RGBカラー紙面補正 | ON | 個別ON/OFF、強さ、背景半径 |
| 傾き補正・湾曲補正 | OFF | 個別ON/OFF |
| 黒インク色差補正・超解像前stroke | OFF | 個別ON/OFF、強さ、除外ページ |
| anime AI内部x2・最終画像x1 | ON | 方式、AI内部倍率、最終画像倍率、model |
| 超解像後stroke | ON、15 | ON/OFF、0–100 |
| 元位置・サイズ保持 | ON | ON/OFF |
| 空白判定と元画像埋め込み | ON | ON/OFF、判定閾値 |

文字内の中間輝度を分離するtone補正とsprite処理は、実験結果を保存するだけに留め、製品パイプラインには入れていません。

## GUI

引数なしで起動し、メイン画面の「本モードを開く...」を選びます。本モード画面では次を実行時に変更でき、最後に実行した値は設定ファイルへ保存されます。

- ScanTailor、crop、背景正規化、傾き補正、湾曲補正のON/OFF
- 余白、入出力DPI、despeckle、ページ検出tolerance
- 超解像のOFF/anime/Lanczos、AI内部倍率、最終画像倍率、model、GPU/CPU worker数、tile、GPU ID、TTA
- 文字太さのON/OFFと強さ
- JPEG品質と4:4:4/4:2:2/4:2:0
- A4上の元位置・サイズ保持のON/OFF
- 中断再開と中間画像保持
- ScanTailor、Real-ESRGAN、model directoryのパス
- 空白判定のON/OFF、暗部とエッジの閾値
- crop除外ページ、カラー紙面補正、黒インク色差補正、超解像前の任意の線補強

## 空白ページ

空白判定は抽出画像の中央90%を256×256で調べ、撮影端や綴じ影の影響を除外します。背景輝度から25以上暗い画素と、輝度差12以上のエッジがどちらも0.05%以下のときだけ空白扱いにします。章扉・奥付・ページ番号だけのページを残すため、判定は意図的に保守的です。

空白ページはページ順を保ち、ScanTailor、超解像、文字太さ調整を省略します。元画像を通常のJPEGとしてPDFへ埋め込むため、薄い紙色や撮影状態も失われません。CLIでは以下を変更できます。

```text
--blank-detection on|off
--blank-dark-delta 25
--blank-max-dark-ratio 0.0005
--blank-edge-threshold 12
--blank-max-edge-ratio 0.0005
```

## 表紙とカラー紙面補正

色面が画像端まで続く表紙はScanTailorが一部分だけを紙面と誤認しやすいため、1ページ目を既定のcrop除外ページにしています。`--crop-exclude-pages 1,158,10-12` のように手動変更でき、空文字を指定すれば除外なしです。

カラー紙面補正は、低周波背景を推定したうえで、明るく低彩度な画素だけをRGB別に補正します。黒い文字や高彩度の図版はmaskから外すため、グレースケール化や全面的な自動levelより色を保ちやすい方式です。

```text
--crop-exclude-pages 1
--color-normalize on|off
--color-normalize-strength 100
--color-normalize-radius 40
```

## 黒インクの色差補正

低品質JPEGや撮影画像では、本来は黒い文字の輪郭に青紫・黄緑などの色差ノイズが残ることがあります。ページ全体をグレースケール化せず、低彩度な暗部を黒文字の核として検出し、その2px以内かつ明るすぎない画素だけを同じ輝度の無彩色へ近づけます。濃紺の見出しや青い帯を「暗いから黒」と判定しないため、暗部すべてを無彩色化する方式は使用しません。

補正は超解像前に行います。文字形状と輝度は保持するため、後段の文字太さ調整とは独立です。写真・表紙などを確実に触らせたくない場合は除外ページを追加できます。1ページ目は既定で除外します。

実ページでは色差以外の中間輝度も読みづらさの原因となり、改善量が限定的だったため既定はOFFです。撮影時に露出、ピント、ホワイトバランスを安定させることを優先し、本補正は色縁が明確なページにだけ使用します。

```text
--ink-neutralize on|off
--ink-neutralize-strength 100
--ink-neutralize-exclude-pages 1
```

欠損線の多いページでは、`--pre-stroke on --pre-stroke-strength 5` で超解像前に弱く線を補えます。通常ページではOFFが既定です。自然さを優先する個別ページでは `--model realesrgan-x4plus --ai-scale 4 --output-scale 1` により、x4結果を元解像度相当へ縮小できます。anime x2より大幅に遅くなります。x4plusへ直接x2推論を指定すると文字配置が崩れるため、AI内部倍率4を明示してください。

## 超解像倍率の定義

「2倍超解像」を次の2つへ分離します。

- **AI内部倍率**（`--ai-scale`）: Real-ESRGANが一時的に生成する倍率。既定はx2。
- **最終画像倍率**（`--output-scale`）: ScanTailor出力に対してPDFへ実際に埋め込む画素倍率。既定はx1。

既定の `AI内部x2 -> 最終画像x1` は、AIで輪郭を再構成したあとLanczosで元画素数へ戻します。入力が既存img2pdfの標準A4画像1654x2339なら、最終解像度と概ね同じファイルサイズを保ちます。PDF上の物理的な位置と大きさはどちらの倍率にも依存せず、`preserve-position`の座標で決まります。

PDF内部にも2倍の画素を残したい場合だけ`--output-scale 2 --ai-scale 2`を使います。x4モデルを使って最終x2にしたい場合は`--output-scale 2 --ai-scale 4`です。旧名`--scale`と`--inference-scale`も互換用に受理しますが、新規設定では使用しません。

## CLI

```bash
img2pdf book-scan INPUT.pdf OUTPUT.pdf \
  --pages 70-85 \
  --scantailor on \
  --crop on \
  --crop-exclude-pages 1 \
  --normalize on \
  --color-normalize on \
  --ink-neutralize off \
  --ink-neutralize-exclude-pages 1 \
  --deskew off \
  --dewarp off \
  --superres anime \
  --ai-scale 2 \
  --output-scale 1 \
  --gpu-workers 4 \
  --stroke on \
  --stroke-strength 15 \
  --jpeg-quality 90 \
  --jpeg-sampling 444 \
  --preserve-position on \
  --resume on
```

全オプションは次で確認できます。

```bash
img2pdf book-scan --help
```

外部ツールは明示パスまたは環境変数で指定できます。

```bash
export IMG2PDF_SCANTAILOR=/path/to/scantailor-cli
export IMG2PDF_REALESRGAN=/path/to/realesrgan-ncnn-vulkan
```

WSLからWindows版Real-ESRGANを使用する場合は `.exe` を指定します。処理対象をWindowsの一時フォルダへworker単位で橋渡しし、検証済み出力だけをWSL側へ戻します。

## 作業フォルダと再開

既定の作業フォルダは出力PDFと同じ場所の `.<output-stem>-book-work` です。

```text
manifest.json
source/
scantailor-input/
scantailor/
preprocessed/
ai-input/
ai-output/
jpeg/
logs/
```

`manifest.json` には各ページの元キャンバス寸法、crop寸法、復元座標、処理段階、試行回数、生成ファイルを保存します。再開時はファイルの存在だけでなく、画像をデコードして期待寸法と一致することを確認します。完了済みPDFと同じ設定が残っていれば即時終了します。

Real-ESRGANの出力は終了コードにかかわらずページ単位で検証し、欠落・破損・寸法不一致のページだけ1回再試行します。

処理中は、前処理、AI入力準備、GPUへの転送、Real-ESRGAN、AI出力確定、JPEG化について `完了ページ数/総ページ数` と割合を表示します。並列処理側ではatomic counterだけを更新し、20ページを超える本では最大約20回へ通知を間引きます。Real-ESRGAN実行中は出力フォルダを1秒間隔で確認するため、GPU推論を直列化せずに進捗を表示できます。

`--keep-work off` では成功後に大きな中間画像を削除し、manifestとlogを残します。

## PDF生成

本モードは既存のメモリ内 `pdf-writer` 経路とは分離されています。JPEGを1ページずつ読み、PDF Image XObjectとcontent streamを直接書くため、全ページのJPEGやPDF全体を同時にメモリへ保持しません。

元位置保持ONでは、ScanTailor projectの `(source_width, source_height, crop_x, crop_y, crop_width, crop_height)` からA4上の配置を計算します。OFFではcrop画像をA4内へ最大化して中央配置します。

## 外部システム

- ScanTailor Advanced: <https://github.com/4lex4/scantailor-advanced>
- Real-ESRGAN ncnn Vulkan: <https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan>
- Poppler: `pdfimages`, `pdfinfo`, `pdftoppm`

ScanTailor AdvancedはGPL-3.0なので、現段階ではアプリへ同梱せずユーザー指定またはPATHから検出します。同梱配布を行う場合は別途ライセンス対応が必要です。
