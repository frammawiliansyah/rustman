#!/usr/bin/env python3
"""Build rustman.ico (Windows) and rustman.icns (macOS) from PNG exports of assets/icon/icon.svg.

Usage: build-icons.py <png-dir> <out-dir>

<png-dir> must contain icon-{16,32,48,64,128,256}.png for the .ico and
icon-mac-{16,32,64,128,256,512,1024}.png for the .icns. Export them from icon.svg with any
SVG renderer (Chrome, Inkscape, rsvg-convert). The "mac" variants keep the artwork inside
about 80% of the canvas, as Apple's icon grid expects. Only the standard library is used.
"""
import struct
import sys
from pathlib import Path


def build_ico(png_dir: Path) -> bytes:
    sizes = [16, 32, 48, 64, 128, 256]
    images = [(s, (png_dir / f"icon-{s}.png").read_bytes()) for s in sizes]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, data = b"", b""
    for size, png in images:
        dim = 0 if size == 256 else size  # 0 means 256 in the ICO format
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), offset + len(data))
        data += png
    return header + entries + data


def build_icns(png_dir: Path) -> bytes:
    # (OSType, pixel size of the PNG to embed)
    chunks = [
        (b"icp4", 16), (b"icp5", 32), (b"icp6", 64), (b"ic07", 128), (b"ic08", 256),
        (b"ic09", 512), (b"ic10", 1024), (b"ic11", 32), (b"ic12", 64), (b"ic13", 256), (b"ic14", 512),
    ]
    body = b""
    for kind, size in chunks:
        png = (png_dir / f"icon-mac-{size}.png").read_bytes()
        body += kind + struct.pack(">I", len(png) + 8) + png
    return b"icns" + struct.pack(">I", len(body) + 8) + body


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    (out / "rustman.ico").write_bytes(build_ico(src))
    (out / "rustman.icns").write_bytes(build_icns(src))
    for n in (256, 512):
        (out / f"icon-{n}.png").write_bytes((src / f"icon-{n}.png").read_bytes())
    print(f"wrote {out}/rustman.ico, rustman.icns, icon-256.png, icon-512.png")


if __name__ == "__main__":
    main()
