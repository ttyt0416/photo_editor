"""Independent structural/pixel checks for Rust-written compressed PSD/PSB.
Uses only Python struct/zlib, not the Rust reader or writer.
"""
from pathlib import Path
import json
import struct
import zlib
from verify_psd_outputs import Reader


def decode(data, width, height, depth, large, mode):
    reader = Reader(data)
    assert reader.number('H') == mode
    row_size = width * depth // 8
    if mode == 0:
        raw = reader.take(row_size * height)
    elif mode == 1:
        sizes = [reader.number('I' if large else 'H') for _ in range(height)]
        raw = bytearray()
        for size in sizes:
            row = Reader(reader.take(size))
            decoded = bytearray()
            while row.at < len(row.data):
                header = row.number('b')
                if header >= 0:
                    decoded.extend(row.take(header + 1))
                elif header != -128:
                    decoded.extend(row.take(1) * (1 - header))
            assert len(decoded) == row_size
            raw.extend(decoded)
    else:
        raw = zlib.decompress(reader.take(len(reader.data) - reader.at))
        if mode == 3:
            output = bytearray()
            for offset in range(0, len(raw), row_size):
                row = raw[offset:offset + row_size]
                if depth == 16:
                    values = struct.unpack('>' + 'H' * width, row)
                    last = 0
                    for value in values:
                        last = (last + value) & 65535
                        output.extend(struct.pack('>H', last))
                else:
                    values = bytearray(row)
                    for i in range(1, len(values)):
                        values[i] = (values[i] + values[i - 1]) & 255
                    if depth == 32:
                        values = bytes(values[b * width + p] for p in range(width) for b in range(4))
                    output.extend(values)
            raw = output
    assert len(raw) == row_size * height
    assert reader.at == len(reader.data)
    code = {8: 'B', 16: 'H', 32: 'f'}[depth]
    return struct.unpack('>' + code * (width * height), raw)


def inspect(path, mode):
    r = Reader(path.read_bytes())
    assert r.take(4) == b'8BPS'
    large = r.number('H') == 2
    r.take(6)
    channels, height, width, depth, color = [r.number(f) for f in ['H', 'I', 'I', 'H', 'H']]
    assert (channels, width, height, color) == (4, 4, 2, 3)
    r.section()
    r.section()
    outer = r.section(large)
    info = outer.section(large)
    if depth != 8:
        assert len(info.data) == 0
        outer.section()
        assert outer.take(8) == b'8BIM' + (b'Lr16' if depth == 16 else b'Lr32')
        info = outer.section(large)
    records = []
    for _ in range(abs(info.number('h'))):
        top, left, bottom, right = [info.number('i') for _ in range(4)]
        entries = [(info.number('h'), info.number('Q' if large else 'I')) for _ in range(info.number('H'))]
        assert info.take(4) == b'8BIM'
        info.take(8)
        extra = info.section()
        mask = extra.section()
        mask_size = None
        if len(mask.data):
            mt, ml, mb, mr = [mask.number('i') for _ in range(4)]
            mask_size = (mr - ml, mb - mt)
        records.append(((right - left, bottom - top), entries, mask_size))
    checked_source = False
    for size, entries, mask_size in records:
        for channel, length in entries:
            w, h = mask_size if channel == -2 else size
            values = decode(info.take(length), w, h, depth, large, mode)
            if channel == 0 and w == 4:
                expected = ([20 + (i % 4) * 40 for i in range(8)] if depth == 8 else
                            [32896 + i % 4 for i in range(8)] if depth == 16 else
                            [2.125 + i % 4 for i in range(8)])
                assert list(values) == expected
                checked_source = True
    assert checked_source
    merged = decode(r.take(len(r.data) - r.at), width, height * channels, depth, large, mode)
    expected = json.loads((path.parent / f'depth-{depth}-merged.json').read_text())
    for c in range(4):
        for i in range(width * height):
            actual = merged[c * width * height + i]
            target = expected[i * 4 + c]
            if c < 3:
                alpha=expected[i*4+3]
                color=target if depth==32 else min(max(target,0),1)
                target=color*alpha+(1-alpha)
            if depth == 32:
                assert abs(actual - target) < 1e-6
            else:
                maximum = 255 if depth == 8 else 65535
                assert abs(actual / maximum - min(max(target, 0), 1)) <= 1 / maximum + 1e-6
    print(f'{path.name}: layers, mask channels, row lengths/prediction and merged pixels verified')


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[2] / 'qa-output/psd-compression'
    for depth in [8, 16, 32]:
        for mode in range(4):
            for extension in ['psd', 'psb']:
                inspect(root / f'depth-{depth}-mode-{mode}.{extension}', mode)
