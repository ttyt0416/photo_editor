"""Independent binary16 decode, unassociation/ICC and project/output float bits."""
from pathlib import Path
import hashlib
import json
import os
import struct
import zipfile
import argparse
import numpy as np
import tifffile
from icc_reference import tags, matrix, encode_srgb
os.environ.setdefault('OPENCV_IO_ENABLE_OPENEXR', '1')
import cv2

BASE = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--bits', type=int, choices=(16, 32, 64), default=16)
bits = parser.parse_args().bits
folder = {16: 'tiff-half', 32: 'tiff-uint32', 64: 'tiff-double'}[bits]
FIXTURES = BASE/'tests/fixtures'/folder
OUTPUT = BASE/'qa-output'/folder
cases = json.loads((FIXTURES/'manifest.json').read_text())
assert len(cases) == 432
destination = matrix(tags((BASE/'data/color/srgb.icc').read_bytes()))
maximum = 0
for case in cases:
    name = case['file']
    path = FIXTURES/name
    assert hashlib.sha256(path.read_bytes()).hexdigest() == case['sha256']
    with tifffile.TiffFile(path) as file:
        page = file.pages[0]
        stored = page.asarray().astype('f8')
        assert page.bitspersample == bits and int(page.sampleformat) == (1 if bits == 32 else 3)
        if bits == 32:
            stored /= 4294967295.
        if case['channels'] == 1:
            stored = stored[:, :, None]
        elif case['planar']:
            stored = np.moveaxis(stored, 0, 2)
        if case['channels'] in (2, 4):
            assert int(page.extrasamples[0]) == (1 if case['associated'] else 2)
        xmp = page.tags[700].value
        if isinstance(xmp, str): xmp = xmp.encode()
        profile = None if case['profile'] == 'none' else page.tags[34675].value
    alpha = stored[:, :, -1] if case['channels'] in (2, 4) else np.ones((3, 17))
    native = stored[:, :, :1 if case['channels'] < 3 else 3]
    if case['associated']:
        numerator = alpha[:, :, None] - native if case['white'] else native
        native = np.divide(numerator, alpha[:, :, None], out=np.zeros_like(native), where=alpha[:, :, None] != 0)
    elif case['white']:
        native = 1. - native
    native = native.astype('f4')
    if case['channels'] < 3:
        native = np.repeat(native, 3, axis=2)
    if case['profile'] == 'linear':
        if case['channels'] < 3:
            expected = encode_srgb(native.astype('f8'))
        else:
            parsed = tags(profile)
            decoded = []
            for c, key in enumerate(('rTRC', 'gTRC', 'bTRC')):
                tag = parsed[key]
                assert tag[:4] == b'para' and struct.unpack_from('>H', tag, 8)[0] == 0
                gamma = struct.unpack_from('>i', tag, 12)[0]/65536
                decoded.append(np.sign(native[:, :, c])*np.abs(native[:, :, c].astype('f8'))**gamma)
            expected = encode_srgb(np.stack(decoded, axis=2)@(np.linalg.inv(destination)@matrix(parsed)).T)
    else:
        expected = native
    if case['orientation'] in (6, 8):
        rotations = -1 if case['orientation'] == 6 else 1
        expected = np.rot90(expected, rotations)
        alpha = np.rot90(alpha, rotations)
    actual = np.fromfile(OUTPUT/(name+'.f32'), '<f4').reshape(*alpha.shape, 4)
    composite = np.fromfile(OUTPUT/(name+'.comp.f32'), '<f4').reshape(*alpha.shape, 4)
    error = np.max(np.abs(actual[:, :, :3]-expected)/np.maximum(1, np.abs(expected)))
    assert error < .0008, (name, error)
    maximum = max(maximum, error)
    np.testing.assert_array_equal(actual[:, :, 3].view('u4'), alpha.astype('f4').view('u4'))
    if case['profile'] != 'linear':
        np.testing.assert_array_equal(actual[:, :, :3].copy().view('u4'), expected.copy().view('u4'))
    expected_composite = actual.copy()
    expected_composite[alpha == 0] = 0
    np.testing.assert_array_equal(composite.view('u4'), expected_composite.view('u4'))
    with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        manifest = json.loads(project.read('manifest.json'))
        assert manifest['version'] == 13 and manifest['layers'][0]['bit_depth'] == 32
        assert project.read('metadata/source.xmp') == xmp
        if profile is not None: assert project.read('metadata/source.icc') == profile
        decoded = cv2.imdecode(np.frombuffer(project.read(f"layers/{manifest['layers'][0]['id']}.exr"), dtype='u1'), cv2.IMREAD_UNCHANGED)
        assert decoded is not None
        np.testing.assert_array_equal(decoded[:, :, [2, 1, 0, 3]].copy().view('u4'), actual.view('u4'))
    with tifffile.TiffFile(OUTPUT/(name+'-working.tiff')) as file:
        np.testing.assert_array_equal(file.asarray().view('u4'), composite.view('u4'))
        assert file.pages[0].tags[34675].value == (BASE/'data/color/srgb.icc').read_bytes()
    decoded = cv2.imread(str(OUTPUT/(name+'-working.exr')), cv2.IMREAD_UNCHANGED)
    assert decoded is not None
    np.testing.assert_array_equal(decoded[:, :, [2, 1, 0, 3]].copy().view('u4'), composite.view('u4'))
kind = 'UINT32' if bits == 32 else f'Binary{bits}'
print(f'{kind} TIFF: {len(cases)} pinned inputs/{len(cases)*51} pixels; gray/RGB/straight+associated alpha/white+black/separate+contiguous/classic+BigTIFF/both endian/raw+Deflate; independent ICC error {maximum:.8g}<0.0008; exact f32-rounded identity/alpha/project ICC+XMP+CRC/OpenCV EXR/working TIFF bits.')
