"""Independent small RGB Photoshop fixtures, generated with only struct/zlib.

Layout follows Adobe's Photoshop File Formats specification. No editor code is
used to encode these fixtures. Colors deliberately exercise 16-bit precision,
negative offsets, float HDR, alpha, and 16/32-bit ZIP prediction.
"""
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parent


def u16(value):
    return struct.pack(">H", value)


def u32(value):
    return struct.pack(">I", value)


def length(value, large):
    return struct.pack(">Q" if large else ">I", value)


def samples16(count):
    values = [0, 65535, 32896, 1024, 257, 50000, 65530, 4]
    return [[values[(index + channel) % 8] for index in range(count)] for channel in range(4)]


def float_samples(count):
    return [[(2.125 + index / 8, -0.25 + index / 16, 0.3456789, (index + 1) / 8)[channel] for index in range(count)] for channel in range(4)]


def raw_plane(values, depth):
    return b"".join(struct.pack(">H" if depth == 16 else ">f", value) for value in values)


def prediction(raw, width, depth):
    rows = []
    for at in range(0, len(raw), width * depth // 8):
        row = raw[at:at + width * depth // 8]
        if depth == 16:
            values = struct.unpack(">" + "H" * width, row)
            rows.append(b"".join(u16((value - (values[i-1] if i else 0)) & 65535) for i, value in enumerate(values)))
        else:
            planes = bytes(row[pixel * 4 + byte] for byte in range(4) for pixel in range(width))
            rows.append(bytes((value - (planes[i-1] if i else 0)) & 255 for i, value in enumerate(planes)))
    return b"".join(rows)


def header(width, height, depth, large):
    return b"8BPS" + u16(2 if large else 1) + bytes(6) + u16(4) + u32(height) + u32(width) + u16(depth) + u16(3) + u32(0) + u32(0)


def layered(depth, large):
    width, height = 4, 2
    channels = samples16(width * height) if depth == 16 else float_samples(width * height)
    raw = [raw_plane(values, depth) for values in channels]
    encoded = [u16(3) + zlib.compress(prediction(plane, width, depth)) for plane in raw]
    extra = u32(0) + u32(0) + bytes(4)
    record = struct.pack(">4i", 1, -1, 3, 3) + u16(4)
    for channel, data in zip([0, 1, 2, -1], encoded):
        record += struct.pack(">h", channel) + length(len(data), large)
    record += b"8BIMnorm" + bytes([255, 0, 0, 0]) + u32(len(extra)) + extra
    info = struct.pack(">h", -1) + record + b"".join(encoded)
    info += bytes((-len(info)) % 4)
    outer = length(0, large) + u32(0) + b"8BIM" + (b"Lr16" if depth == 16 else b"Lr32") + length(len(info), large) + info
    # The merged preview is separately filled with zeros; layered decoding must use real layers.
    merged = u16(2) + zlib.compress(bytes(5 * 3 * 4 * depth // 8))
    path = ROOT / ("independent-layered16.psd" if depth == 16 else "independent-layered32.psb")
    path.write_bytes(header(5, 3, depth, large) + length(len(outer), large) + outer + merged)


def flattened():
    width, height, depth = 4, 2, 32
    planes = [prediction(raw_plane(values, depth), width, depth) for values in float_samples(width * height)]
    (ROOT / "independent-flat32.psd").write_bytes(header(width, height, depth, False) + u32(0) + u16(3) + zlib.compress(b"".join(planes)))


if __name__ == "__main__":
    layered(16, False)
    layered(32, True)
    flattened()
