#!/usr/bin/env python3
"""Re-typeset extracted public prose/formulas with a controlled LuaLaTeX template."""

from __future__ import annotations

import argparse
import json
import random
import re
import subprocess
from pathlib import Path


def escape_tex(value: str) -> str:
    replacements = {
        "\\": r"\textbackslash{}", "{": r"\{", "}": r"\}", "$": r"\$", "&": r"\&",
        "#": r"\#", "%": r"\%", "_": r"\_", "^": r"\textasciicircum{}", "~": r"\textasciitilde{}",
    }
    return "".join(replacements.get(char, char) for char in value)


def sentence_excerpt(paragraph: str, rng: random.Random, minimum: int = 140, maximum: int = 300) -> str:
    """Choose a sentence-aligned excerpt instead of cutting through Japanese words."""
    sentences = [part.strip() for part in re.split(r"(?<=[。！？])", paragraph) if part.strip()]
    if not sentences:
        return paragraph[:maximum].strip()
    start = rng.randrange(len(sentences))
    selected: list[str] = []
    size = 0
    for sentence in sentences[start:] + sentences[:start]:
        if selected and size + len(sentence) > maximum:
            break
        selected.append(sentence)
        size += len(sentence)
        if size >= minimum:
            break
    return "".join(selected)[:maximum].strip()


def make_document(paragraphs: list[str], formulas: list[str], pages: int, seed: int, jp_font: str, math_font: str) -> str:
    rng = random.Random(seed)
    substantial_formulas = [
        formula for formula in formulas
        if len(formula) >= 14 and ("=" in formula or "\\" in formula or "<" in formula or ">" in formula)
    ]
    if substantial_formulas:
        formulas = substantial_formulas
    body = []
    for page in range(pages):
        if page:
            body.append(r"\newpage")
        body.append(rf"\section*{{合成資料 {page + 1}}}")
        formula_slots = set(rng.sample(range(5), 3))
        for slot in range(5):
            paragraph = rng.choice(paragraphs)
            excerpt = sentence_excerpt(paragraph, rng)
            body.append(escape_tex(excerpt) + r"\par")
            if slot in formula_slots:
                body.append(r"\[" + rng.choice(formulas) + r"\]")
        body.append(r"\small 文字内部の空隙、濁点、添字、細い横画を保持し、見えていない線を過剰に作らない。")
    return rf"""\documentclass[a4paper,10pt]{{ltjsarticle}}
\usepackage[margin=18mm]{{geometry}}
\usepackage{{luatexja-fontspec,unicode-math,amsmath}}
\setmainjfont{{{jp_font}}}
\setmainfont{{TeX Gyre Termes}}
\setmathfont{{{math_font}}}
\pagestyle{{plain}}
\setlength{{\parindent}}{{1em}}
\setlength{{\parskip}}{{4pt}}
\begin{{document}}
{chr(10).join(body)}
\end{{document}}
"""


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus_dir", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--pages", type=int, default=40)
    parser.add_argument("--dpi", type=int, default=200, help="raster resolution (200 dpi matches the current book-scan working scale)")
    parser.add_argument("--jp-font", default="IPAexMincho")
    parser.add_argument("--math-font", default="TeX Gyre Termes Math")
    parser.add_argument("--seed", type=int, default=20260902)
    args = parser.parse_args()
    paragraphs = [line for line in (args.corpus_dir / "japanese.txt").read_text().splitlines() if len(line) >= 80]
    formulas = json.loads((args.corpus_dir / "formulas.json").read_text())
    if not paragraphs or not formulas:
        raise SystemExit("corpus is empty; run fetch_public_corpora.py first")
    args.output.mkdir(parents=True, exist_ok=True)
    tex_path = args.output / "public-corpus.tex"
    tex_path.write_text(make_document(paragraphs, formulas, args.pages, args.seed, args.jp_font, args.math_font))
    subprocess.run(
        ["lualatex", "-no-shell-escape", "-interaction=nonstopmode", "-halt-on-error", tex_path.name],
        cwd=args.output,
        check=True,
    )
    raster = args.output / "clean"
    raster.mkdir(exist_ok=True)
    subprocess.run(
        ["pdftoppm", "-png", "-r", str(args.dpi), str(args.output / "public-corpus.pdf"), str(raster / "public")],
        check=True,
    )
    print(f"clean_images={raster}")


if __name__ == "__main__":
    main()
