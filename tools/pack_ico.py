"""Pack PNG and single-image ICO files into one Windows .ico file.

Usage: pack_ico.py OUTPUT INPUT...

PNG inputs are embedded unchanged (compressed); use them for the 256 px
entry. Single-image ICO inputs (BMP entries, as written by ImageMagick) are
copied as-is, keeping small sizes compatible with every Windows consumer.
Entries are written largest first.
"""

import struct
import sys

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def read_entry(path):
    with open(path, "rb") as file:
        data = file.read()
    if data.startswith(PNG_SIGNATURE):
        if data[12:16] != b"IHDR":
            sys.exit(f"{path}: malformed PNG")
        width, height = struct.unpack(">II", data[16:24])
        bit_depth, color_type = data[24], data[25]
        if (bit_depth, color_type) != (8, 6):
            sys.exit(f"{path}: PNG entries must be 8-bit RGBA")
        return width, height, 32, data
    reserved, kind, count = struct.unpack("<HHH", data[:6])
    if reserved != 0 or kind != 1 or count != 1:
        sys.exit(f"{path}: expected a single-image ICO file")
    width, height, _, _, _, bit_count, size, offset = struct.unpack("<BBBBHHII", data[6:22])
    return width or 256, height or 256, bit_count, data[offset : offset + size]


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    entries = sorted((read_entry(path) for path in sys.argv[2:]), key=lambda entry: -entry[0])
    sizes = [entry[0] for entry in entries]
    if len(set(sizes)) != len(sizes):
        sys.exit(f"duplicate icon sizes: {sizes}")
    if any(not 1 <= width <= 256 or width != height for width, height, _, _ in entries):
        sys.exit(f"icon entries must be square and at most 256 px: {sizes}")

    header = struct.pack("<HHH", 0, 1, len(entries))
    directory = b""
    payload = b""
    offset = len(header) + 16 * len(entries)
    for width, height, bit_count, data in entries:
        # A stored size of 0 means 256.
        directory += struct.pack("<BBBBHHII", width % 256, height % 256, 0, 0, 1, bit_count, len(data), offset + len(payload))
        payload += data
    with open(sys.argv[1], "wb") as file:
        file.write(header + directory + payload)


if __name__ == "__main__":
    main()
