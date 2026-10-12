"""Independent JP2 UUID/XMP/project checks and native OpenJPEG quality layers."""
from pathlib import Path
import json
import struct
import zipfile
import xml.etree.ElementTree as ET
import numpy as np
from jpeg2000_c_reference import components, version

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'qa-output/jpeg2000-xmp'
UUID = bytes.fromhex('be7acfcb97a942e89c71999491e3afac')
TIFF = 'http://ns.adobe.com/tiff/1.0/'
EXIF = 'http://ns.adobe.com/exif/1.0/'


def boxes(data):
    at = 0
    while at < len(data):
        size, kind = struct.unpack_from('>I4s', data, at)
        header = 8
        if size == 1:
            size = struct.unpack_from('>Q', data, at + 8)[0]
            header = 16
        elif size == 0:
            size = len(data) - at
        assert size >= header and at + size <= len(data)
        yield kind, data[at + header:at + size], at
        at += size
    assert at == len(data)


def packets(data):
    return [(body[16:], offset) for kind, body, offset in boxes(data)
            if kind == b'uuid' and body[:16] == UUID]


source = (OUT / 'source.xmp').read_bytes()
assert len(source) > 65535
cases = json.loads((OUT / 'manifest.json').read_text())
assert len(cases) == 128
pixels = 0
pairs = 0
partial_layers = 0
for case in cases:
    path = OUT / case['file']
    data = path.read_bytes()
    xmp = packets(data)
    assert len(xmp) == int(case['include'])
    if xmp:
        packet, offset = xmp[0]
        assert data[offset + 4:offset + 8] == b'uuid'
        assert list(boxes(data))[-1][2] == offset
        tree = ET.fromstring(packet)
        expected = {
            f'{{{TIFF}}}Orientation': '1',
            f'{{{TIFF}}}ImageWidth': str(case['w']),
            f'{{{TIFF}}}ImageLength': str(case['h']),
            f'{{{EXIF}}}PixelXDimension': str(case['w']),
            f'{{{EXIF}}}PixelYDimension': str(case['h']),
        }
        assert tree.attrib == expected
        assert tree.find('{urn:qa}creator').text == '촬영자'
        assert b'<!--' + b'opaque' * 12000 + b'-->' in packet
        other = OUT / case['file'].replace('-xtrue.jp2', '-xfalse.jp2')
        assert data[:offset] == other.read_bytes(), case['file']
        for layer in range(1, 5):
            np.testing.assert_array_equal(components(path, layer), components(other, layer))
            partial_layers += 1
        pairs += 1
    planes = components(path)
    count = 1 if case['gray'] else 3
    assert planes.shape == (case['h'], case['w'], count + int(case['alpha']))
    limit = (1 << case['depth']) - 1
    color = np.clip(planes[:, :, :count], 0, limit)
    if case['gray']:
        color = np.repeat(color, 3, axis=2)
    alpha = np.clip(planes[:, :, count], 0, limit) if case['alpha'] else np.full(color.shape[:2], limit)
    expected = np.concatenate([color, alpha[:, :, None]], axis=2).astype('f4') / np.float32(limit)
    expected[:, :, :3][alpha == 0] = 0
    raw = np.fromfile(OUT / case['raw'], dtype='<f4').reshape(case['h'], case['w'], 4)
    np.testing.assert_array_equal(raw, expected)
    with zipfile.ZipFile(OUT / (case['file'] + '.rphoto')) as project:
        assert project.testzip() is None
        manifest = json.loads(project.read('manifest.json'))
        if xmp:
            assert manifest['version'] == 13
            assert project.read('metadata/source.xmp') == xmp[0][0]
        else:
            assert 'metadata/source.xmp' not in project.namelist()
    pixels += case['w'] * case['h']

base = ROOT / 'tests/fixtures/independent.jp2'
for mode in range(3):
    path = OUT / f'inputs/length-{mode}.jp2'
    assert packets(path.read_bytes())[0][0] == source
    np.testing.assert_array_equal(components(path), components(base))
    with zipfile.ZipFile(OUT / f'inputs/length-{mode}.rphoto') as project:
        assert project.testzip() is None
        assert project.read('metadata/source.xmp') == source
        assert json.loads(project.read('manifest.json'))['version'] == 13
np.testing.assert_array_equal(components(OUT / 'inputs/duplicate.jp2'), components(base))
assert (OUT / 'errors/raw-true.j2k').read_bytes() == (OUT / 'errors/raw-false.j2k').read_bytes()
print(f'JP2 XMP: C OpenJPEG {version}, {len(cases)} files/{pixels} pixels, {pairs} exact entire-image-prefix pairs and {partial_layers} quality-layer comparisons; native RGB/gray8/16, alpha, lossless/lossy, 128-pixel edge tiles/ICC/300ppi; UUID namespace values, >64KB Unicode/opaque packet/project CRC, 3 source box lengths and duplicate pixels, raw J2K unchanged.')
