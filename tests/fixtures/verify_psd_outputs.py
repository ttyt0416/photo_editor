"""Independent inspection of native output produced by the Rust integration tests."""
from pathlib import Path
import struct


class Reader:
    def __init__(self, data):
        self.data = memoryview(data)
        self.at = 0

    def take(self, count):
        value = self.data[self.at:self.at + count]
        if len(value) != count:
            raise ValueError("Truncated PSD")
        self.at += count
        return bytes(value)

    def number(self, form):
        return struct.unpack(">" + form, self.take(struct.calcsize(">" + form)))[0]

    def section(self, large=False):
        return Reader(self.take(self.number("Q" if large else "I")))


def inspect(path, expected_depth):
    reader = Reader(path.read_bytes())
    assert reader.take(4) == b"8BPS"
    large = reader.number("H") == 2
    reader.take(6)
    channels, height, width, depth, mode = [reader.number(form) for form in ["H", "I", "I", "H", "H"]]
    assert (channels, width, height, depth, mode) == (4, 5, 3, expected_depth, 3)
    reader.section()
    reader.section()
    outer = reader.section(large)
    assert len(outer.section(large).data) == 0
    outer.section()
    assert outer.take(8) == b"8BIM" + (b"Lr16" if depth == 16 else b"Lr32")
    info = outer.section(large)
    count = abs(info.number("h"))
    records = []
    for _ in range(count):
        top, left, bottom, right = [info.number("i") for _ in range(4)]
        channel_count = info.number("H")
        channel_records = [(info.number("h"), info.number("Q" if large else "I")) for _ in range(channel_count)]
        assert info.take(4) == b"8BIM"
        info.take(8)
        info.section()
        records.append(((top, left, bottom, right), channel_records))
    source = None
    for bounds, channel_records in records:
        for channel, size in channel_records:
            data = Reader(info.take(size))
            assert data.number("H") == 0
            if bounds == (1, -1, 3, 3) and channel == 0:
                source = [data.number("H" if depth == 16 else "f") for _ in range(8)]
    assert source == ([32896 + index % 4 for index in range(8)] if depth == 16 else [2.125 + index % 4 for index in range(8)])
    assert reader.number("H") == 0
    merged = [[reader.number("H" if depth == 16 else "f") for _ in range(width * height)] for _ in range(4)]
    assert not any(merged[3][:width])
    alpha = 0.75 * 192 / 255 * 128 / 255
    if depth == 32:
        assert abs(merged[3][6] - alpha) < 1e-6
        assert abs(merged[0][6] - (4.125 * (1 - 128 / 255)*alpha+(1-alpha))) < 1e-6
    assert reader.at == len(reader.data)
    print(f"{path.name}: native channels, Lr{depth}, groups and merged preview verified")


if __name__ == "__main__":
    root = Path(__file__).resolve().parents[2] / "qa-output/high-psd"
    for depth in [16, 32]:
        for extension in ["psd", "psb"]:
            inspect(root / f"native-{depth}.{extension}", depth)
