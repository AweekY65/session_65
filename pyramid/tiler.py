"""Local image pyramid + tile builder.

Everything is stored on the local filesystem (or in memory): no map server,
CDN, cloud storage or any other external service is involved.

Layout of an output directory::

    out/
      manifest.json
      tiles/<level>/<x>/<y>.png

Edge tiles are *cropped* to the real content (no padding); their actual
width/height is recorded in the manifest.

All writes go through a temporary file in the target directory followed by
``os.replace`` (atomic on POSIX), and the manifest is written last, so an
interrupted run never leaves a half-written file that the manifest treats
as valid.
"""

from __future__ import annotations

import hashlib
import json
import os
import tempfile
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from PIL import Image

from .resample import RESAMPLERS

MANIFEST_NAME = "manifest.json"
MANIFEST_VERSION = 1
TMP_PREFIX = ".tmp-"


@dataclass(frozen=True)
class PyramidConfig:
    tile_size: int = 256
    min_size: int = 256
    resample: str = "bilinear"

    def __post_init__(self):
        if self.tile_size < 1:
            raise ValueError("tile_size must be >= 1")
        if self.min_size < 1:
            raise ValueError("min_size must be >= 1")
        if self.resample not in RESAMPLERS:
            raise ValueError(f"unknown resample method: {self.resample!r}")

    def as_dict(self) -> dict:
        return {
            "tile_size": self.tile_size,
            "min_size": self.min_size,
            "resample": self.resample,
        }


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def compute_level_sizes(width: int, height: int, min_size: int) -> list[tuple[int, int]]:
    """Level 0 is the original; each level halves (ceil) until max(w,h) <= min_size."""
    sizes = [(width, height)]
    while max(width, height) > min_size:
        width = max(1, (width + 1) // 2)
        height = max(1, (height + 1) // 2)
        sizes.append((width, height))
    return sizes


def tile_grid(width: int, height: int, tile_size: int) -> list[tuple[int, int, int, int]]:
    """Return (x, y, tile_w, tile_h) for every tile; edge tiles are cropped."""
    tiles = []
    for y in range(0, height, tile_size):
        for x in range(0, width, tile_size):
            tiles.append((x, y, min(tile_size, width - x), min(tile_size, height - y)))
    return tiles


def tile_relpath(level: int, tx: int, ty: int) -> str:
    return f"tiles/{level}/{tx}/{ty}.png"


def _encode_png(arr: np.ndarray) -> bytes:
    """Lossless, deterministic PNG encoding."""
    import io

    buf = io.BytesIO()
    Image.fromarray(arr, mode="RGB").save(buf, format="PNG", optimize=False)
    return buf.getvalue()


def _atomic_write(path: Path, data: bytes) -> None:
    """Write via temp file in the same directory + atomic rename."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(dir=path.parent, prefix=TMP_PREFIX)
    try:
        with os.fdopen(fd, "wb") as fh:
            fh.write(data)
            fh.flush()
            os.fsync(fh.fileno())
        os.replace(tmp_name, path)
    except BaseException:
        try:
            os.unlink(tmp_name)
        except OSError:
            pass
        raise


def _cleanup_stale_temps(out_dir: Path) -> int:
    removed = 0
    if not out_dir.is_dir():
        return 0
    for root, _dirs, files in os.walk(out_dir):
        for name in files:
            if name.startswith(TMP_PREFIX):
                try:
                    os.unlink(Path(root) / name)
                    removed += 1
                except OSError:
                    pass
    return removed


def load_image_rgb(path: Path) -> np.ndarray:
    with Image.open(path) as img:
        return np.asarray(img.convert("RGB"), dtype=np.uint8)


def _load_manifest(out_dir: Path) -> dict | None:
    manifest_path = out_dir / MANIFEST_NAME
    if not manifest_path.is_file():
        return None
    try:
        with open(manifest_path, "r", encoding="utf-8") as fh:
            return json.load(fh)
    except (OSError, json.JSONDecodeError):
        return None


def _manifest_matches(manifest: dict, source_sha: str, width: int, height: int,
                      config: PyramidConfig) -> bool:
    try:
        return (
            manifest.get("version") == MANIFEST_VERSION
            and manifest["source"]["sha256"] == source_sha
            and manifest["source"]["width"] == width
            and manifest["source"]["height"] == height
            and manifest["config"] == config.as_dict()
        )
    except (KeyError, TypeError):
        return False


def _expected_tiles(width: int, height: int, config: PyramidConfig) -> list[dict]:
    """Full expected tile plan for every level."""
    levels = []
    for level, (lw, lh) in enumerate(compute_level_sizes(width, height, config.min_size)):
        tiles = []
        for x, y, tw, th in tile_grid(lw, lh, config.tile_size):
            tx, ty = x // config.tile_size, y // config.tile_size
            tiles.append({
                "x": tx, "y": ty, "width": tw, "height": th,
                "path": tile_relpath(level, tx, ty),
            })
        levels.append({
            "level": level, "width": lw, "height": lh,
            "tiles_x": (lw + config.tile_size - 1) // config.tile_size,
            "tiles_y": (lh + config.tile_size - 1) // config.tile_size,
            "tiles": tiles,
        })
    return levels


def _tile_is_valid(out_dir: Path, manifest: dict | None, level_idx: int,
                   tile: dict) -> bool:
    """A tile is reusable only if the manifest records it and the on-disk
    file exists with a matching sha256."""
    if manifest is None:
        return False
    try:
        recorded = manifest["levels"][level_idx]["tiles"]
        entry = next(t for t in recorded
                     if t["x"] == tile["x"] and t["y"] == tile["y"])
        if (entry["width"] != tile["width"] or entry["height"] != tile["height"]
                or entry["path"] != tile["path"]):
            return False
        tile_path = out_dir / tile["path"]
        return tile_path.is_file() and sha256_file(tile_path) == entry["sha256"]
    except (KeyError, IndexError, StopIteration, TypeError, OSError):
        return False


def build_pyramid(source: str | Path, out_dir: str | Path,
                  config: PyramidConfig | None = None) -> dict:
    """Build (or incrementally rebuild) the pyramid. Returns a stats dict."""
    config = config or PyramidConfig()
    source = Path(source)
    out_dir = Path(out_dir)
    if not source.is_file():
        raise FileNotFoundError(f"source image not found: {source}")

    _cleanup_stale_temps(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    source_sha = sha256_file(source)
    image = load_image_rgb(source)
    height, width = image.shape[0], image.shape[1]

    old_manifest = _load_manifest(out_dir)
    if old_manifest is not None and not _manifest_matches(
            old_manifest, source_sha, width, height, config):
        old_manifest = None  # source or config changed: nothing is reusable

    levels = _expected_tiles(width, height, config)

    # Decide which tiles must be (re)generated.
    invalid: dict[int, list[dict]] = {}
    reused = 0
    for level_idx, level in enumerate(levels):
        bad = [t for t in level["tiles"]
               if not _tile_is_valid(out_dir, old_manifest, level_idx, t)]
        if bad:
            invalid[level_idx] = bad
        reused += len(level["tiles"]) - len(bad)

    written = 0
    if invalid:
        resample = RESAMPLERS[config.resample]
        level_img = image
        deepest = max(invalid)
        for level_idx in range(deepest + 1):
            if level_idx > 0:
                lw, lh = levels[level_idx]["width"], levels[level_idx]["height"]
                level_img = resample(level_img, lw, lh)
            for tile in invalid.get(level_idx, ()):
                x0 = tile["x"] * config.tile_size
                y0 = tile["y"] * config.tile_size
                patch = level_img[y0:y0 + tile["height"], x0:x0 + tile["width"]]
                png = _encode_png(patch)
                tile["sha256"] = sha256_bytes(png)
                _atomic_write(out_dir / tile["path"], png)
                written += 1

    # Fill hashes of reused tiles from the old manifest.
    if old_manifest is not None:
        for level_idx, level in enumerate(levels):
            recorded = old_manifest["levels"][level_idx]["tiles"]
            for tile in level["tiles"]:
                if "sha256" not in tile:
                    entry = next(t for t in recorded
                                 if t["x"] == tile["x"] and t["y"] == tile["y"])
                    tile["sha256"] = entry["sha256"]

    manifest = {
        "version": MANIFEST_VERSION,
        "source": {
            "path": str(source),
            "sha256": source_sha,
            "width": width,
            "height": height,
        },
        "config": config.as_dict(),
        "levels": levels,
    }
    payload = json.dumps(manifest, indent=2, sort_keys=False).encode("utf-8")
    _atomic_write(out_dir / MANIFEST_NAME, payload)

    return {
        "source": str(source),
        "out_dir": str(out_dir),
        "width": width,
        "height": height,
        "levels": len(levels),
        "tiles_total": sum(len(l["tiles"]) for l in levels),
        "tiles_written": written,
        "tiles_reused": reused,
        "up_to_date": written == 0,
    }


def validate_output(out_dir: str | Path) -> list[str]:
    """Verify manifest consistency and every tile hash. Returns a list of
    problems (empty list means the output is fully valid)."""
    out_dir = Path(out_dir)
    problems: list[str] = []
    manifest = _load_manifest(out_dir)
    if manifest is None:
        return ["manifest.json missing or unreadable"]
    for level in manifest.get("levels", []):
        for tile in level.get("tiles", []):
            path = out_dir / tile["path"]
            if not path.is_file():
                problems.append(f"missing tile: {tile['path']}")
                continue
            if sha256_file(path) != tile.get("sha256"):
                problems.append(f"hash mismatch: {tile['path']}")
    return problems
