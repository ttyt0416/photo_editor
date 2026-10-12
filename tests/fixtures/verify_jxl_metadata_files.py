"""Independent JXL container payloads, TIFF bytes, projects and reference pixels."""
from pathlib import Path
from PIL import Image
import json
import struct
import zipfile
import zlib
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'qa-output/jxl-metadata'
SIGNATURE = b'\0\0\0\x0cJXL \x0d\x0a\x87\x0a'


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
        yield kind, data[at + header:at + size]
        at += size
    assert at == len(data)


def codestream(data):
    if data.startswith(b'\xff\x0a'):
        return data
    return b''.join(body if kind == b'jxlc' else body[4:]
                    for kind, body in boxes(data) if kind in [b'jxlc', b'jxlp'])


cases = json.loads((OUT / 'manifest.json').read_text())
assert len(cases) == 12
source = (OUT / 'source.xmp').read_bytes()
pixels = 0
for case in cases:
    data = (OUT / case['file']).read_bytes()
    assert data.startswith(SIGNATURE)
    parts = list(boxes(data))
    xmp = [body for kind, body in parts if kind == b'xml ']
    exif = [body for kind, body in parts if kind == b'Exif']
    assert xmp == [source] and len(exif) == 1
    offset = struct.unpack_from('>I', exif[0])[0]
    assert offset == (6 if case['before'] else 0)
    raw_exif = exif[0][4 + offset:]
    assert raw_exif == (OUT / case['exif']).read_bytes()
    parsed = Image.Exif()
    parsed.load(raw_exif)
    assert parsed[274] == 8 and parsed[271] == 'QA Camera'
    assert raw_exif[:2] == (b'II' if case['before'] else b'MM')
    types = [kind for kind, body in parts]
    assert (types.index(b'Exif') < types.index(b'jxlc')) == case['before']
    baseline = (ROOT / f"tests/fixtures/jxl/{case['name']}/input.jxl").read_bytes()
    assert codestream(data) == codestream(baseline)
    raw = np.fromfile(OUT / case['raw'], dtype='<f4').reshape(case['h'], case['w'], 4)
    official = np.frombuffer(zlib.decompress((ROOT / f"qa-output/jxl/{case['name']}.rgba32.le.zlib").read_bytes()), dtype='<f4').reshape(case['h'], case['w'], 4)
    decoded = np.fromfile(OUT / case['decoded'], dtype='<f4').reshape(case['h'], case['w'], 4)
    np.testing.assert_array_equal(decoded, official)
    composite = official.copy()
    composite[:, :, :3][composite[:, :, 3] == 0] = 0
    np.testing.assert_array_equal(raw, composite)
    with zipfile.ZipFile(OUT / (case['file'] + '.rphoto')) as project:
        assert project.testzip() is None
        manifest = json.loads(project.read('manifest.json'))
        assert manifest['version'] == 13
        assert project.read('metadata/source.xmp') == source
        assert project.read('metadata/source.exif') == raw_exif
    pixels += case['w'] * case['h']
split = (OUT / 'errors/split.jxl').read_bytes()
base = (ROOT / 'tests/fixtures/jxl/grayscale_jpeg/input.jxl').read_bytes()
assert codestream(split) == codestream(base)
assert [kind for kind, body in boxes(split)].count(b'jxlp') == 2
print(f'JXL metadata: {len(cases)} files/{pixels} pixels from all 6 pinned libjxl references; exact codestream/reference floats and project CRC/v13 EXIF/XMP bytes, metadata before+after image, normal/extended/end-to-file boxes, little+big TIFF offset0/6/Make/orientation8 without repeated rotation, split jxlp and unrelated box.')
