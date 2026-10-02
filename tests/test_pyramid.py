"""Automated tests for the local image pyramid builder.

All checks run in the terminal; no image windows are opened. Test images are
generated programmatically with a fixed seed so results are reproducible.
"""

from __future__ import annotations

import json
import os
import time
from pathlib import Path

import numpy as np
import pytest
from PIL import Image

from pyramid.resample import resize_bilinear, resize_nearest
from pyramid.tiler import (
    MANIFEST_NAME,
    PyramidConfig,
    _atomic_write,
    build_pyramid,
    compute_level_sizes,
    load_image_rgb,
    sha256_file,
    tile_grid,
    validate_output,
)

FAST = PyramidConfig(tile_size=64, min_size=32, resample="bilinear")


def make_test_image(path: Path, width: int, height: int, fmt: str = "PNG",
                    seed: int = 42) -> Path:
    """Deterministic gradient + pseudo-random noise test image."""
    rng = np.random.default_rng(seed)
    yy, xx = np.mgrid[0:height, 0:width]
    arr = np.stack([
        (xx * 255 // max(width - 1, 1)),
        (yy * 255 // max(height - 1, 1)),
        ((xx + yy) % 256),
    ], axis=-1).astype(np.uint8)
    noise = rng.integers(0, 32, size=(height, width, 1), dtype=np.uint8)
    arr = np.clip(arr.astype(np.int32) + noise, 0, 255).astype(np.uint8)
    Image.fromarray(arr, "RGB").save(path, format=fmt)
    return path


def read_manifest(out_dir: Path) -> dict:
    with open(out_dir / MANIFEST_NAME, "r", encoding="utf-8") as fh:
        return json.load(fh)


def check_ok(out_dir: Path) -> None:
    problems = validate_output(out_dir)
    assert problems == [], f"output invalid: {problems}"
    print(f"  [check] {out_dir.name}: manifest + tile hashes OK")


# ---------------------------------------------------------------- level math

def test_level_sizes_odd_dimensions():
    sizes = compute_level_sizes(333, 257, 32)
    assert sizes[0] == (333, 257)
    assert sizes[1] == (167, 129)  # ceil halving
    assert sizes[-1][0] <= 32 and sizes[-1][1] <= 32
    for (w, h), (nw, nh) in zip(sizes, sizes[1:]):
        assert nw == max(1, (w + 1) // 2)
        assert nh == max(1, (h + 1) // 2)
    print(f"  [check] odd-size levels: {sizes}")


def test_level_sizes_stop_condition():
    assert compute_level_sizes(256, 256, 256) == [(256, 256)]
    assert compute_level_sizes(256, 100, 256) == [(256, 100)]
    assert compute_level_sizes(257, 100, 256) == [(257, 100), (129, 50)]
    assert compute_level_sizes(1, 1, 1) == [(1, 1)]


def test_tile_grid_edge_crop():
    tiles = tile_grid(300, 200, 128)
    assert len(tiles) == 3 * 2
    edge = [t for t in tiles if t[0] == 256]
    assert edge == [(256, 0, 44, 128), (256, 128, 44, 72)]
    corner = [t for t in tiles if t[0] == 256 and t[1] == 128]
    assert corner[0][2:] == (44, 72)  # cropped, not padded
    print("  [check] edge tiles cropped to real size (44x72 corner)")


# ------------------------------------------------------------------ resample

def test_resample_methods_differ_and_are_deterministic(tmp_path):
    src = make_test_image(tmp_path / "src.png", 97, 61)
    arr = load_image_rgb(src)
    near = resize_nearest(arr, 33, 21)
    bil = resize_bilinear(arr, 33, 21)
    assert near.shape == bil.shape == (21, 33, 3)
    assert not np.array_equal(near, bil)
    # determinism: identical inputs -> identical outputs
    assert np.array_equal(resize_nearest(arr, 33, 21), near)
    assert np.array_equal(resize_bilinear(arr, 33, 21), bil)
    print("  [check] nearest != bilinear, both deterministic")


# ------------------------------------------------------------ build / output

@pytest.mark.parametrize("resample", ["nearest", "bilinear"])
def test_build_odd_size_image(tmp_path, resample):
    src = make_test_image(tmp_path / "odd.png", 333, 257)
    out = tmp_path / f"out_{resample}"
    config = PyramidConfig(tile_size=64, min_size=32, resample=resample)
    stats = build_pyramid(src, out, config)

    manifest = read_manifest(out)
    assert manifest["source"]["width"] == 333
    assert manifest["source"]["height"] == 257
    assert manifest["config"]["resample"] == resample
    assert manifest["levels"][0]["width"] == 333

    # every recorded tile exists, has the cropped size and matching hash
    for level in manifest["levels"]:
        for tile in level["tiles"]:
            path = out / tile["path"]
            assert path.is_file()
            with Image.open(path) as img:
                assert img.size == (tile["width"], tile["height"])
            assert len(tile["sha256"]) == 64
    check_ok(out)
    print(f"  [check] {resample}: {stats['levels']} levels, "
          f"{stats['tiles_total']} tiles, all cropped sizes correct")


def test_supports_jpeg_input(tmp_path):
    src = make_test_image(tmp_path / "photo.jpg", 200, 150, fmt="JPEG")
    out = tmp_path / "out_jpg"
    stats = build_pyramid(src, out, FAST)
    assert stats["tiles_total"] > 0
    check_ok(out)
    print("  [check] JPEG input OK")


def test_level1_tiles_match_original_pixels(tmp_path):
    src = make_test_image(tmp_path / "exact.png", 100, 70)
    out = tmp_path / "out_exact"
    build_pyramid(src, out, FAST)
    original = load_image_rgb(src)
    manifest = read_manifest(out)
    for tile in manifest["levels"][0]["tiles"]:
        with Image.open(out / tile["path"]) as img:
            patch = np.asarray(img, dtype=np.uint8)
        x0, y0 = tile["x"] * 64, tile["y"] * 64
        assert np.array_equal(
            patch, original[y0:y0 + tile["height"], x0:x0 + tile["width"]])
    print("  [check] level-0 tiles reassemble the original exactly")


def test_same_input_stable_output(tmp_path):
    src = make_test_image(tmp_path / "stable.png", 150, 111)
    out_a, out_b = tmp_path / "a", tmp_path / "b"
    build_pyramid(src, out_a, FAST)
    build_pyramid(src, out_b, FAST)
    ma, mb = read_manifest(out_a), read_manifest(out_b)
    ha = [t["sha256"] for l in ma["levels"] for t in l["tiles"]]
    hb = [t["sha256"] for l in mb["levels"] for t in l["tiles"]]
    assert ha == hb
    print(f"  [check] identical input -> identical {len(ha)} tile hashes")


# --------------------------------------------------------------- incremental

def test_incremental_no_rewrite_when_unchanged(tmp_path):
    src = make_test_image(tmp_path / "inc.png", 200, 140)
    out = tmp_path / "out_inc"
    stats1 = build_pyramid(src, out, FAST)
    assert stats1["tiles_written"] == stats1["tiles_total"]

    mtimes = {p: p.stat().st_mtime_ns for p in out.rglob("*.png")}
    time.sleep(0.01)
    stats2 = build_pyramid(src, out, FAST)
    assert stats2["up_to_date"] and stats2["tiles_written"] == 0
    assert mtimes == {p: p.stat().st_mtime_ns for p in out.rglob("*.png")}
    check_ok(out)
    print(f"  [check] unchanged input: 0 of {stats1['tiles_total']} tiles rewritten")


def test_corrupt_tile_is_regenerated(tmp_path):
    src = make_test_image(tmp_path / "corrupt.png", 200, 140)
    out = tmp_path / "out_corrupt"
    build_pyramid(src, out, FAST)
    manifest = read_manifest(out)
    victim = manifest["levels"][0]["tiles"][0]
    victim_path = out / victim["path"]
    good_hash = sha256_file(victim_path)
    victim_path.write_bytes(b"corrupted!")
    assert sha256_file(victim_path) != good_hash

    stats = build_pyramid(src, out, FAST)
    assert stats["tiles_written"] == 1
    assert sha256_file(victim_path) == good_hash
    check_ok(out)
    print(f"  [check] corrupted tile regenerated: {victim['path']}")


def test_missing_tile_is_regenerated(tmp_path):
    src = make_test_image(tmp_path / "missing.png", 200, 140)
    out = tmp_path / "out_missing"
    build_pyramid(src, out, FAST)
    manifest = read_manifest(out)
    victim = manifest["levels"][1]["tiles"][-1]
    (out / victim["path"]).unlink()

    stats = build_pyramid(src, out, FAST)
    assert stats["tiles_written"] == 1
    assert (out / victim["path"]).is_file()
    check_ok(out)
    print(f"  [check] missing tile regenerated: {victim['path']}")


def test_tampered_manifest_entry_triggers_regeneration(tmp_path):
    src = make_test_image(tmp_path / "tamper.png", 150, 90)
    out = tmp_path / "out_tamper"
    build_pyramid(src, out, FAST)
    manifest = read_manifest(out)
    manifest["levels"][0]["tiles"][0]["sha256"] = "0" * 64
    (out / MANIFEST_NAME).write_text(json.dumps(manifest), encoding="utf-8")

    stats = build_pyramid(src, out, FAST)
    assert stats["tiles_written"] == 1
    check_ok(out)
    print("  [check] tampered manifest hash -> tile rebuilt")


# ------------------------------------------------------- atomicity / failure

def test_interrupted_build_leaves_no_valid_half_files(tmp_path, monkeypatch):
    src = make_test_image(tmp_path / "crash.png", 200, 140)
    out = tmp_path / "out_crash"

    real_write = _atomic_write
    state = {"count": 0}

    def flaky_write(path, data):
        state["count"] += 1
        if state["count"] == 3:
            # simulate a crash halfway through the third write
            path.parent.mkdir(parents=True, exist_ok=True)
            with open(path, "wb") as fh:
                fh.write(data[: len(data) // 2])  # partial junk at final name
            raise RuntimeError("simulated crash")
        return real_write(path, data)

    import pyramid.tiler as tiler
    monkeypatch.setattr(tiler, "_atomic_write", flaky_write)
    with pytest.raises(RuntimeError):
        build_pyramid(src, out, FAST)
    monkeypatch.undo()

    # the manifest must not certify the interrupted state
    assert not (out / MANIFEST_NAME).exists()
    # a fresh run recovers and produces a fully valid output
    stats = build_pyramid(src, out, FAST)
    assert stats["tiles_written"] >= 1
    check_ok(out)
    leftovers = [p for p in out.rglob("*") if p.name.startswith(".tmp-")]
    assert leftovers == []
    print("  [check] crash mid-write: no manifest, recovery run valid, "
          "no temp leftovers")


def test_stale_temp_files_cleaned_on_next_run(tmp_path):
    src = make_test_image(tmp_path / "temp.png", 100, 80)
    out = tmp_path / "out_temp"
    build_pyramid(src, out, FAST)
    stale = out / "tiles" / "0" / ".tmp-abandoned"
    stale.write_bytes(b"half file")
    build_pyramid(src, out, FAST)
    assert not stale.exists()
    check_ok(out)
    print("  [check] stale temp files cleaned")


def test_atomic_write_replaces_atomically(tmp_path):
    target = tmp_path / "sub" / "file.bin"
    _atomic_write(target, b"one")
    _atomic_write(target, b"two")
    assert target.read_bytes() == b"two"
    assert list(tmp_path.rglob(".tmp-*")) == []
    print("  [check] atomic write: temp + replace, no leftovers")
