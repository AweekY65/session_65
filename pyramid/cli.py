"""Command line interface: build a local image pyramid + tiles.

Example:
    python -m pyramid.cli input.png -o out/ --tile-size 256 --resample bilinear
"""

from __future__ import annotations

import argparse
import sys

from .tiler import PyramidConfig, build_pyramid, validate_output


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="pyramid",
        description="Build a local image pyramid with tiles and a manifest. "
                    "Everything stays on the local filesystem.",
    )
    parser.add_argument("source", nargs="?", help="input image (PNG/JPEG)")
    parser.add_argument("-o", "--out", required=True, help="output directory")
    parser.add_argument("--tile-size", type=int, default=256)
    parser.add_argument("--min-size", type=int, default=256,
                        help="stop when max(width, height) <= min-size")
    parser.add_argument("--resample", choices=["nearest", "bilinear"],
                        default="bilinear")
    parser.add_argument("--validate", action="store_true",
                        help="only validate an existing output directory")
    args = parser.parse_args(argv)

    if args.validate:
        problems = validate_output(args.out)
        if problems:
            for p in problems:
                print(f"INVALID: {p}")
            return 1
        print("OK: manifest and all tile hashes verified")
        return 0

    if not args.source:
        parser.error('source is required unless --validate is given')
    config = PyramidConfig(tile_size=args.tile_size, min_size=args.min_size,
                           resample=args.resample)
    stats = build_pyramid(args.source, args.out, config)
    print(f"source      : {stats['source']} ({stats['width']}x{stats['height']})")
    print(f"output      : {stats['out_dir']}")
    print(f"levels      : {stats['levels']}")
    print(f"tiles total : {stats['tiles_total']}")
    print(f"tiles written: {stats['tiles_written']}")
    print(f"tiles reused : {stats['tiles_reused']}")
    print("status      : " + ("up to date (no tiles regenerated)"
                              if stats["up_to_date"] else "rebuilt"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
