"""Deterministic image resampling (nearest / bilinear) implemented with numpy.

Both resamplers use the pixel-center convention:

    src = (dst + 0.5) * (in_size / out_size) - 0.5

and round-half-up when converting back to integers, so results are stable
across runs for identical inputs (pure float64 numpy math, no randomness,
no dependency on Pillow's internal resampling filters).
"""

from __future__ import annotations

import numpy as np


def _check_array(arr: np.ndarray) -> np.ndarray:
    if arr.ndim != 3 or arr.shape[2] != 3:
        raise ValueError("expected an (H, W, 3) RGB array")
    if arr.dtype != np.uint8:
        raise ValueError("expected uint8 array")
    return arr


def _check_size(out_w: int, out_h: int) -> None:
    if out_w < 1 or out_h < 1:
        raise ValueError("output size must be >= 1x1")


def _src_coords(in_size: int, out_size: int) -> np.ndarray:
    """Source coordinates (float64) for each output pixel along one axis."""
    return (np.arange(out_size, dtype=np.float64) + 0.5) * (in_size / out_size) - 0.5


def resize_nearest(arr: np.ndarray, out_w: int, out_h: int) -> np.ndarray:
    """Nearest-neighbour resize with round-half-up tie breaking."""
    _check_array(arr)
    _check_size(out_w, out_h)
    in_h, in_w = arr.shape[0], arr.shape[1]

    xs = np.clip(np.floor(_src_coords(in_w, out_w) + 0.5), 0, in_w - 1).astype(np.int64)
    ys = np.clip(np.floor(_src_coords(in_h, out_h) + 0.5), 0, in_h - 1).astype(np.int64)
    return arr[np.ix_(ys, xs)].copy()


def resize_bilinear(arr: np.ndarray, out_w: int, out_h: int) -> np.ndarray:
    """Bilinear resize (float64 accumulation, round-half-up to uint8)."""
    _check_array(arr)
    _check_size(out_w, out_h)
    in_h, in_w = arr.shape[0], arr.shape[1]

    def axis_weights(in_size: int, out_size: int):
        coords = np.clip(_src_coords(in_size, out_size), 0.0, float(in_size - 1))
        lo = np.floor(coords).astype(np.int64)
        hi = np.minimum(lo + 1, in_size - 1)
        frac = (coords - lo).reshape(-1, 1)  # (out_size, 1)
        return lo, hi, frac

    x_lo, x_hi, x_frac = axis_weights(in_w, out_w)
    y_lo, y_hi, y_frac = axis_weights(in_h, out_h)

    src = arr.astype(np.float64)
    # Interpolate along x for the two row candidates, then along y.
    top = src[y_lo][:, x_lo] * (1.0 - x_frac)[None, :, :] + src[y_lo][:, x_hi] * x_frac[None, :, :]
    bot = src[y_hi][:, x_lo] * (1.0 - x_frac)[None, :, :] + src[y_hi][:, x_hi] * x_frac[None, :, :]
    out = top * (1.0 - y_frac)[:, :, None] + bot * y_frac[:, :, None]

    return np.clip(np.floor(out + 0.5), 0, 255).astype(np.uint8)


RESAMPLERS = {
    "nearest": resize_nearest,
    "bilinear": resize_bilinear,
}
