#!/usr/bin/env python3
"""Fetch license-filtered Japanese prose and safely extract public TeX formulas."""

from __future__ import annotations

import argparse
import csv
from datetime import date
import io
import json
import random
import re
import subprocess
import urllib.request
import zipfile
from pathlib import Path


AOZORA_INDEX = "https://www.aozora.gr.jp/index_pages/list_person_all_extended_utf8.zip"
OPENLOGIC_REPO = "https://github.com/OpenLogicProject/OpenLogic.git"
FORBIDDEN_TEX = re.compile(
    r"\\(?:input|include|write|openout|read|catcode|usepackage|documentclass|newcommand|renewcommand|def|edef|csname|special|immediate)\b"
)
ALLOWED_COMMANDS = {
    "alpha", "beta", "gamma", "delta", "epsilon", "varepsilon", "zeta", "eta", "theta", "vartheta",
    "iota", "kappa", "lambda", "mu", "nu", "xi", "pi", "varpi", "rho", "varrho", "sigma", "tau",
    "upsilon", "phi", "varphi", "chi", "psi", "omega", "Gamma", "Delta", "Theta", "Lambda", "Xi",
    "Pi", "Sigma", "Upsilon", "Phi", "Psi", "Omega", "mathbb", "mathbf", "mathrm", "mathcal", "mathfrak",
    "operatorname", "text", "frac", "dfrac", "tfrac", "sqrt", "binom", "sum", "prod", "coprod", "int",
    "iint", "iiint", "oint", "lim", "limsup", "liminf", "sup", "inf", "max", "min", "log", "ln", "exp",
    "sin", "cos", "tan", "det", "ker", "dim", "gcd", "mod", "pmod", "bmod", "partial", "nabla", "infty",
    "to", "rightarrow", "leftarrow", "leftrightarrow", "Rightarrow", "Leftarrow", "Leftrightarrow", "mapsto",
    "longrightarrow", "hookrightarrow", "twoheadrightarrow", "uparrow", "downarrow", "left", "right", "bigl",
    "bigr", "Bigl", "Bigr", "langle", "rangle", "lfloor", "rfloor", "lceil", "rceil", "vert", "mid",
    "in", "notin", "ni", "subset", "supset", "subseteq", "supseteq", "cup", "cap", "setminus", "emptyset",
    "varnothing", "forall", "exists", "neg", "land", "lor", "vdash", "models", "le", "leq", "ge", "geq",
    "neq", "equiv", "approx", "simeq", "sim", "cong", "propto", "perp", "parallel", "pm", "mp", "times",
    "div", "cdot", "circ", "ast", "star", "oplus", "otimes", "wedge", "vee", "triangle", "triangleleft",
    "overline", "underline", "widehat", "widetilde", "hat", "tilde", "bar", "vec", "dot", "ddot", "dots",
    "ldots", "cdots", "vdots", "ddots", "quad", "qquad", "enspace", "!", ",", ";", ":", "prime",
}


def download(url: str) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": "img2pdf-deblur-corpus/1.0"})
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read()


def clean_aozora(text: str) -> str:
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    separators = list(re.finditer(r"^-{20,}\s*$", text, re.MULTILINE))
    if len(separators) >= 2:
        text = text[separators[1].end() :]
    text = re.split(r"\n底本：", text, maxsplit=1)[0]
    text = re.sub(r"※?［＃.*?］", "", text)
    text = re.sub(r"《([^》]+)》", "", text)
    text = text.replace("｜", "")
    paragraphs = []
    for line in text.splitlines():
        line = line.strip().replace("　", " ")
        if len(line) >= 20 and not line.startswith(("底本", "入力", "校正")):
            paragraphs.append(line)
    return "\n".join(paragraphs)


def fetch_aozora(output: Path, count: int, seed: int, include_old: bool) -> list[dict]:
    archive = zipfile.ZipFile(io.BytesIO(download(AOZORA_INDEX)))
    csv_name = archive.namelist()[0]
    rows = list(csv.DictReader(io.StringIO(archive.read(csv_name).decode("utf-8-sig"))))
    grouped: dict[str, list[dict]] = {}
    for row in rows:
        grouped.setdefault(row["作品ID"], []).append(row)
    candidates = []
    for work_rows in grouped.values():
        row = work_rows[0]
        if not row["テキストファイルURL"]:
            continue
        if any(item["作品著作権フラグ"] != "なし" or item["人物著作権フラグ"] != "なし" for item in work_rows):
            continue
        if not include_old and row["文字遣い種別"] != "新字新仮名":
            continue
        candidates.append(row)
    random.Random(seed).shuffle(candidates)
    paragraphs, sources = [], []
    for row in candidates:
        if len(sources) >= count:
            break
        try:
            payload = download(row["テキストファイルURL"])
            with zipfile.ZipFile(io.BytesIO(payload)) as item_zip:
                name = next(name for name in item_zip.namelist() if name.lower().endswith(".txt"))
                raw = item_zip.read(name)
            encoding = "cp932" if "shift" in row["テキストファイル符号化方式"].lower() else "utf-8"
            cleaned = clean_aozora(raw.decode(encoding, errors="replace"))
            if len(cleaned) < 500:
                continue
            paragraphs.extend(cleaned[:30000].splitlines())
            sources.append(
                {
                    "work_id": row["作品ID"],
                    "title": row["作品名"],
                    "author": row["姓"] + row["名"],
                    "url": row["図書カードURL"],
                    "text_url": row["テキストファイルURL"],
                    "copyright": "expired (青空文庫の作品著作権フラグ・人物著作権フラグともになし)",
                }
            )
            print(f"aozora {len(sources)}/{count}: {row['作品名']} / {row['姓']}{row['名']}", flush=True)
        except Exception as error:
            print(f"aozora skip {row['作品ID']}: {error}")
    (output / "japanese.txt").write_text("\n".join(paragraphs) + "\n")
    return sources


def balanced_braces(value: str) -> bool:
    depth = 0
    for char in value:
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth < 0:
                return False
    return depth == 0


def safe_formula(value: str) -> str | None:
    value = re.sub(r"\s+", " ", value).strip()
    if (
        not 4 <= len(value) <= 180
        or "&" in value
        or "^^" in value
        or FORBIDDEN_TEX.search(value)
        or not balanced_braces(value)
    ):
        return None
    commands = set(re.findall(r"\\([A-Za-z]+|[,;:!])", value))
    if not commands.issubset(ALLOWED_COMMANDS):
        return None
    return value


def extract_openlogic(repo: Path, maximum: int, seed: int) -> tuple[list[str], list[dict]]:
    formulas = set()
    patterns = [
        re.compile(r"\\\[(.+?)\\\]", re.DOTALL),
        re.compile(r"\\begin\{(?:equation\*?|displaymath)\}(.+?)\\end\{(?:equation\*?|displaymath)\}", re.DOTALL),
        re.compile(r"(?<!\\)\$(?!\$)(.+?)(?<!\\)\$", re.DOTALL),
    ]
    files = sorted((repo / "content").rglob("*.tex"))
    for path in files:
        text = path.read_text(errors="ignore")
        text = re.sub(r"(?<!\\)%.*", "", text)
        for pattern in patterns:
            for match in pattern.finditer(text):
                formula = safe_formula(match.group(1))
                if formula:
                    formulas.add(formula)
    values = sorted(formulas)
    random.Random(seed).shuffle(values)
    values = values[:maximum]
    source = [{"name": "Open Logic Project", "url": "https://github.com/OpenLogicProject/OpenLogic", "license": "CC BY 4.0"}]
    return values, source


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--aozora-count", type=int, default=30)
    parser.add_argument("--openlogic-count", type=int, default=3000)
    parser.add_argument("--openlogic-repo", type=Path)
    parser.add_argument("--include-old-orthography", action="store_true")
    parser.add_argument("--seed", type=int, default=20260902)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    repo = args.openlogic_repo or (args.output / "source" / "openlogic")
    if not repo.exists():
        repo.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "clone", "--depth", "1", OPENLOGIC_REPO, str(repo)], check=True)
    aozora_sources = fetch_aozora(args.output, args.aozora_count, args.seed, args.include_old_orthography)
    formulas, math_sources = extract_openlogic(repo, args.openlogic_count, args.seed)
    (args.output / "formulas.json").write_text(json.dumps(formulas, ensure_ascii=False, indent=2) + "\n")
    attribution = {
        "generated": date.today().isoformat(),
        "aozora_index": AOZORA_INDEX,
        "japanese_sources": aozora_sources,
        "math_sources": math_sources,
        "notes": "arXiv is deliberately excluded by default because most submissions only grant arXiv a non-exclusive distribution license.",
    }
    (args.output / "ATTRIBUTION.json").write_text(json.dumps(attribution, ensure_ascii=False, indent=2) + "\n")
    print(f"japanese_paragraphs={len((args.output / 'japanese.txt').read_text().splitlines())}")
    print(f"safe_formulas={len(formulas)}")


if __name__ == "__main__":
    main()
