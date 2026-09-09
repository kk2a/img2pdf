#!/usr/bin/env python3
"""Generate colour-preserving synthetic blur pairs from clean reference images."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

import cv2
import numpy as np


EXTENSIONS = {".jpg", ".jpeg", ".png", ".tif", ".tiff", ".webp", ".bmp"}


def image_files(root: Path) -> list[Path]:
    if root.is_file():
        return [root]
    return sorted(path for path in root.rglob("*") if path.suffix.lower() in EXTENSIONS)


def seed_for(seed: int, relative: str, variant: int) -> int:
    digest = hashlib.sha256(f"{seed}:{relative}:{variant}".encode()).digest()
    return int.from_bytes(digest[:8], "little")


def degrade(
    clean: np.ndarray,
    rng: np.random.Generator,
    sigma_min: float,
    sigma_max: float,
    scale_min: float,
    scale_max: float,
    noise_max: float,
    jpeg_probability: float,
) -> tuple[np.ndarray, dict[str, float]]:
    """Apply only colour-neutral blur/resampling/noise; no tint or illumination changes."""
    sigma = float(rng.uniform(sigma_min, sigma_max))
    scale = float(rng.uniform(scale_min, scale_max))
    image = cv2.GaussianBlur(clean, (0, 0), sigma, borderType=cv2.BORDER_REFLECT)
    height, width = image.shape[:2]
    small = cv2.resize(
        image,
        (max(8, round(width * scale)), max(8, round(height * scale))),
        interpolation=cv2.INTER_AREA,
    )
    interpolation = int(rng.choice([cv2.INTER_CUBIC, cv2.INTER_LANCZOS4, cv2.INTER_LINEAR]))
    image = cv2.resize(small, (width, height), interpolation=interpolation).astype(np.float32)
    noise_sigma = float(rng.uniform(0.0, noise_max))
    # Luma noise is shared by RGB channels and therefore cannot create coloured fringes.
    image += rng.normal(0.0, noise_sigma, (height, width, 1)).astype(np.float32)
    image = np.uint8(np.clip(np.rint(image), 0, 255))
    jpeg_quality = 0
    if rng.random() < jpeg_probability:
        jpeg_quality = int(rng.integers(68, 96))
        ok, encoded = cv2.imencode(".jpg", image, [cv2.IMWRITE_JPEG_QUALITY, jpeg_quality])
        if not ok:
            raise RuntimeError("JPEG encoding failed")
        image = cv2.imdecode(encoded, cv2.IMREAD_COLOR)
    return image, {
        "sigma": sigma,
        "scale": scale,
        "noise_sigma": noise_sigma,
        "jpeg_quality": jpeg_quality,
    }


def prepare(args: argparse.Namespace) -> Path:
    sources = image_files(args.clean_dir)
    if not sources:
        raise SystemExit(f"no clean images found: {args.clean_dir}")
    output = args.output.resolve()
    gt_dir = output / "gt"
    input_dir = output / "input"
    gt_dir.mkdir(parents=True, exist_ok=True)
    input_dir.mkdir(parents=True, exist_ok=True)
    manifest = []
    for image_index, path in enumerate(sources, 1):
        relative = path.name if args.clean_dir.is_file() else path.relative_to(args.clean_dir).as_posix()
        slug = hashlib.sha1(relative.encode()).hexdigest()[:10] + "-" + path.stem
        clean = cv2.imread(str(path), cv2.IMREAD_COLOR)
        if clean is None:
            print(f"skip unreadable image: {path}")
            continue
        target_path = gt_dir / f"{slug}.png"
        cv2.imwrite(str(target_path), clean)
        split_value = int.from_bytes(hashlib.sha1(relative.encode()).digest()[:4], "little") % 100
        split = "validation" if split_value < args.validation_percent else "train"
        for variant in range(args.variants):
            rng = np.random.default_rng(seed_for(args.seed, relative, variant))
            degraded, parameters = degrade(
                clean,
                rng,
                args.sigma_min,
                args.sigma_max,
                args.scale_min,
                args.scale_max,
                args.noise_max,
                args.jpeg_probability,
            )
            degraded_path = input_dir / f"{slug}-v{variant:02d}.png"
            cv2.imwrite(str(degraded_path), degraded)
            manifest.append(
                {
                    "split": split,
                    "input": degraded_path.relative_to(output).as_posix(),
                    "target": target_path.relative_to(output).as_posix(),
                    "source": str(path.resolve()),
                    "degradation": parameters,
                }
            )
        print(f"[{image_index}/{len(sources)}] {split}: {relative}", flush=True)
    manifest_path = output / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    print(f"pairs={len(manifest)} manifest={manifest_path}")
    return manifest_path


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument("clean_dir", type=Path, help="clean GT image or directory")
    result.add_argument("output", type=Path, help="generated dataset directory")
    result.add_argument("--variants", type=int, default=4)
    result.add_argument("--validation-percent", type=int, default=10)
    result.add_argument("--sigma-min", type=float, default=0.55)
    result.add_argument("--sigma-max", type=float, default=2.10)
    result.add_argument("--scale-min", type=float, default=0.72)
    result.add_argument("--scale-max", type=float, default=0.98)
    result.add_argument("--noise-max", type=float, default=1.2)
    result.add_argument("--jpeg-probability", type=float, default=0.15)
    result.add_argument("--seed", type=int, default=20260902)
    return result


def main() -> None:
    args = parser().parse_args()
    if not 0 <= args.validation_percent < 100:
        raise SystemExit("validation-percent must be in [0, 100)")
    if not 0 < args.sigma_min <= args.sigma_max:
        raise SystemExit("invalid sigma range")
    if not 0 < args.scale_min <= args.scale_max <= 1:
        raise SystemExit("invalid scale range")
    prepare(args)


if __name__ == "__main__":
    main()
