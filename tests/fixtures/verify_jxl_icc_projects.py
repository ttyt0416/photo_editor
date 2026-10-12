"""Pinned libjxl source ICC hashes, reference floats and independent ZIP assets."""
from pathlib import Path
import hashlib
import io
import json
import struct
import zipfile
import zlib
import brotli
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tests/fixtures/jxl-icc'
OUT = ROOT / 'qa-output/jxl-icc'
manifest = json.loads((FIXTURES / 'manifest.json').read_text())
for name, entry in manifest['files'].items():
    data = (FIXTURES / name).read_bytes()
    assert len(data) == entry['size'] and hashlib.sha256(data).hexdigest() == entry['sha256']
    if 'original_sha256' in entry:
        original = zlib.decompress(data) if name.endswith('.zlib') else data
        assert len(original) == entry['original_size']
        assert hashlib.sha256(original).hexdigest() == entry['original_sha256']
cases = json.loads((OUT / 'manifest.json').read_text())
assert len(cases) == 4
pixels = 0
maximum = 0
versions = set()


def source_metadata(data):
    if data.startswith(b'\xff\x0a'):
        return {}
    at = 0
    assets = {}
    while at < len(data):
        size, kind = struct.unpack_from('>I4s', data, at)
        header = 8
        if size == 1:
            size = struct.unpack_from('>Q', data, at + 8)[0]
            header = 16
        elif size == 0:
            size = len(data) - at
        assert size >= header and at + size <= len(data)
        body = data[at + header:at + size]
        if kind == b'brob':
            kind, body = body[:4], brotli.decompress(body[4:])
        if kind == b'Exif':
            offset = struct.unpack_from('>I', body)[0]
            assets['metadata/source.exif'] = body[4 + offset:]
        elif kind == b'xml ':
            assets['metadata/source.xmp'] = body
        at += size
    assert at == len(data)
    return assets


def curve(profile, tag):
    for index in range(struct.unpack_from('>I', profile, 128)[0]):
        at = 132 + 12 * index
        if profile[at:at + 4] == tag:
            offset, length = struct.unpack_from('>II', profile, at + 4)
            data = profile[offset:offset + length]
            assert data[:4] == b'para'
            function = struct.unpack_from('>H', data, 8)[0]
            params = np.array(struct.unpack('>' + str((len(data) - 12) // 4) + 'i', data[12:])) / 65536
            return function, params
    raise AssertionError('missing curve')


for case in cases:
    folder = FIXTURES / case['name']
    descriptor = json.loads((folder / 'test.json').read_text())
    profile = (folder / 'original.icc').read_bytes()
    assert hashlib.sha256(profile).hexdigest() == descriptor['sha256sums']['original.icc']
    reference_data = zlib.decompress((folder / 'reference_image.npy.zlib').read_bytes())
    assert hashlib.sha256(reference_data).hexdigest() == descriptor['sha256sums']['reference_image.npy']
    reference = np.load(io.BytesIO(reference_data), allow_pickle=False)[0]
    assert list(reference.shape) == manifest['cases'][case['name']]['shape'][1:]
    actual = np.fromfile(OUT / case['raw'], dtype='<f4').reshape(case['h'], case['w'], 4)
    assert reference.shape[:2] == actual.shape[:2]
    assert np.all(actual[:, :, 3] == 1)
    expected = np.repeat(reference, 3, axis=2) if reference.shape[2] == 1 else reference
    rendered = (OUT / f"{case['name']}.rendered.icc").read_bytes()
    if case['name'] == 'grayscale':
        # The decoder returns sRGB gray while this corpus uses linear gray.
        # Interpret each through its actual rendered ICC, preserving the
        # separate original ICC bytes. Extend the standard curve at negatives.
        assert rendered[16:20] == b'GRAY'
        function, p = curve(rendered, b'kTRC')
        assert function in [3, 4]
        if function == 3:
            p = np.concatenate([p, [0, 0]])
        assert len(p) == 7
        g, a, b, c, d, e, f = p
        expected_params = np.array([2.4, 1 / 1.055, .055 / 1.055, 1 / 12.92, .04045, 0, 0])
        assert np.max(np.abs(p - expected_params)) < 1 / 65536
        target_function, target = curve((folder / 'reference.icc').read_bytes(), b'kTRC')
        assert target_function == 3
        np.testing.assert_array_equal(target, [1, 1, 0, 1, 0])
        actual = actual.astype('f8')
        v = actual[:, :, :3]
        actual[:, :, :3] = np.where(v >= d, np.power(a * np.maximum(v, d) + b, g) + e, c * v + f)
    quant = {8: 0.5 / 255, 16: 0.5 / 65535, 32: 0}[case['depth']]
    error = np.abs(actual[:, :, :3].astype('f8') - expected.astype('f8'))
    peak = error.max(axis=(0, 1))
    rms = np.sqrt(np.mean(error * error, axis=(0, 1)))
    assert np.all(peak <= descriptor['frames'][0]['peak_error'] + quant + 1e-6), (case, peak)
    assert np.all(rms <= descriptor['frames'][0]['rms_error'] + quant + 1e-7), (case, rms)
    maximum = max(maximum, float(error.max()))
    assets = source_metadata((OUT / f"{case['name']}-{str(case['metadata']).lower()}.jxl").read_bytes())
    with zipfile.ZipFile(OUT / f"{case['name']}-{str(case['metadata']).lower()}.rphoto") as project:
        assert project.testzip() is None
        record = json.loads(project.read('manifest.json'))
        assert record['version'] == (13 if assets else 12)
        versions.add(record['version'])
        assert project.read('metadata/source.icc') == profile
        for asset, raw in assets.items():
            assert project.read(asset) == raw
    pixels += case['w'] * case['h']
with zipfile.ZipFile(OUT / 'implicit.rphoto') as project:
    assert 'metadata/source.icc' not in project.namelist()
    assert json.loads(project.read('manifest.json'))['version'] == 8
assert (FIXTURES / 'grayscale/original.icc').read_bytes() != (FIXTURES / 'grayscale/reference.icc').read_bytes()
assert versions == {12, 13}
print(f'JXL original ICC: 4 inputs/{pixels} pixels, pinned corpus exact original RGB/gray profile bytes/SHA-256 and independent reference floats (peak {maximum:.9g}), ICC-only v12/combined v13 project CRC, implicit profile stays v8; original gray differs from rendered profile and is preserved without reapplication.')
