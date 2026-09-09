# Document deblur training tools

NAF-DPMの決定論的`init_predictor`を、色を変えないGaussian系の合成劣化または自前のclean画像から再学習するための補助ツールです。Rust本体とは独立しており、実験後にONNXモデルを本体へ組み込むことを想定しています。

## 方針

- clean画像は一切補正せずGTとして保存する。
- 既定劣化はGaussian blur、縮小再標本化、輝度noise、低確率のJPEGだけ。色かぶり・照明補正・自動grayは行わない。
- train/validationは元画像単位で分ける。同じclean画像の別variantが両方へ入ることはない。
- 日本語を左右反転するaugmentationは行わない。
- blank pageも約10%の確率で学習patchへ残す。
- `--ink-weight`で文字線の欠落をやや強く罰する。過度に太る場合は0へ戻す。

Python依存は`torch`, `numpy`, `opencv-python`、検証時は`scikit-image`、ONNX出力時はさらに`onnx`です。別途[NAF-DPM公式repository](https://github.com/ispamm/NAF-DPM)と公式`BEST_PSNR_model_init_800000.pth`が必要です。

## 手持ちの正解画像から一括学習

clean PNG/JPEGを`CLEAN_DIR`へ置きます。フォルダは再帰的に読みます。

```bash
python tools/deblur_training/train_pipeline.py CLEAN_DIR .work/my-deblur \
  --naf-repo .work/models/NAF-DPM \
  --initial-weights .work/models/BEST_PSNR_model_init_800000.pth \
  --variants 4 --steps 1200
```

生成物:

- `.work/my-deblur/dataset/manifest.json`: input/GT対応と劣化parameter
- `.work/my-deblur/naf-document-gaussian.pth`: PyTorch predictor
- `.work/my-deblur/naf-document-gaussian-256.onnx`: batchだけ可変の256x256 ONNX

劣化の強さは`--sigma-max`と`--scale-min`で変えられます。現在の推奨初期値はsigma 0.55--2.10、scale 0.72--0.98です。

## 適用

1画像:

```bash
python tools/deblur_training/apply_predictor.py input.jpg output.png \
  --naf-repo .work/models/NAF-DPM \
  --weights .work/my-deblur/naf-document-gaussian.pth
```

フォルダ全体:

```bash
python tools/deblur_training/apply_predictor.py input-pages output-pages \
  --naf-repo .work/models/NAF-DPM \
  --weights .work/my-deblur/naf-document-gaussian.pth \
  --strength 0.8
```

出力は元解像度・元縦横比のPNGです。`--strength 0.0..1.0`で入力とのblend量を変えられます。

合成validationへのPSNR/SSIMと、RGB色差だけを取り出した`chroma_mae`は次のように比較できます。

```bash
python tools/deblur_training/evaluate_predictor.py \
  .work/my-deblur/dataset/manifest.json .work/my-deblur/metrics.json \
  --naf-repo .work/models/NAF-DPM \
  --model base=.work/models/BEST_PSNR_model_init_800000.pth \
  --model tuned=.work/my-deblur/naf-document-gaussian.pth
```

## 公開文章・公開TeXからclean画像を作る

既定候補は次の2つです。

- [青空文庫](https://www.aozora.gr.jp/guide/kijyunn.html): 公式索引で作品・人物の著作権フラグがともに「なし」の作品だけを取得。自然な日本語用。
- [Open Logic Project](https://github.com/OpenLogicProject/OpenLogic): CC BY 4.0。論理・集合・証明のTeX数式用。

```bash
python tools/deblur_training/fetch_public_corpora.py .work/public-corpus \
  --aozora-count 30 \
  --openlogic-repo .work/book-scan/corpora/openlogic

python tools/deblur_training/render_public_corpus.py \
  .work/public-corpus .work/public-render --pages 40
```

`render_public_corpus.py`が作る`.work/public-render/clean`を`train_pipeline.py`へ渡せます。取得元・作品名・URLは`ATTRIBUTION.json`へ残ります。

公開TeXをそのままcompileしてはいけません。取得器は標準的な数式commandだけをwhitelist抽出し、固定templateを`-no-shell-escape`で再組版します。独自macro、`\input`、`\write`等は捨てます。

### 採用しなかった既定source

- arXiv: [公式bulk data説明](https://info.arxiv.org/help/bulk_data_s3.html)にある通り、大半の投稿はarXivへの非独占的配布許諾だけで、第三者へ再配布する権利は与えていない。OAI-PMH metadataでCC0/CC BY等を個別filterする実装ができるまで既定取得しない。
- Stacks Project: TeXは豊富だがGNU FDLで、生成datasetを配るときの扱いがCC BY/MITより複雑なためopt-in候補。
- Wikipedia: 日本語量は多いがCC BY-SAの帰属・継承をdataset/modelでどう扱うかを先に決める必要がある。
- CTAN: packageごとにlicenseが異なるため、CTAN全体を一括sourceにはできない。
- BCCWJ: 高品質だが利用契約・配布条件があり、自動取得用の既定sourceにはしない。

MATH datasetの[公式repository](https://github.com/hendrycks/math)はMITで有力ですが、現行repositoryはloader中心でdataset本体を外部mirrorから取得する形です。取得元とchecksumを固定してから追加します。
