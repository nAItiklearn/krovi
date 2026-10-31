"""Generate Krovi placeholder icons (original, brand-teal) with stdlib only.
Writes a 128x128 RGBA PNG and a PNG-embedded .ico. Minimal on purpose — a real
icon set is a packaging-phase task.
"""
import struct
import sys
import zlib


def solid_png(w: int, h: int, rgba: tuple) -> bytes:
    row = bytes(rgba) * w
    raw = bytearray()
    for _ in range(h):
        raw.append(0)  # filter type 0 (none)
        raw.extend(row)
    comp = zlib.compress(bytes(raw), 9)

    def chunk(typ: bytes, data: bytes) -> bytes:
        body = typ + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", comp)
    png += chunk(b"IEND", b"")
    return png


def ico_from_png(png: bytes, size: int) -> bytes:
    dim = 0 if size >= 256 else size
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), 6 + 16)
    return header + entry + png


def main() -> None:
    out_dir = sys.argv[1]
    teal = (0x0E, 0x9E, 0x8F, 0xFF)
    png = solid_png(128, 128, teal)
    with open(f"{out_dir}/icon.png", "wb") as f:
        f.write(png)
    with open(f"{out_dir}/128x128.png", "wb") as f:
        f.write(png)
    with open(f"{out_dir}/32x32.png", "wb") as f:
        f.write(solid_png(32, 32, teal))
    with open(f"{out_dir}/icon.ico", "wb") as f:
        f.write(ico_from_png(png, 128))
    print("wrote icon.png, 128x128.png, 32x32.png, icon.ico")


if __name__ == "__main__":
    main()
