"""Independent TIFF chunks: NumPy cumsum predictions, edge cropping and outputs."""
from pathlib import Path
import hashlib
import json
import os
import zipfile
import zlib
import argparse
import numpy as np
import tifffile
os.environ.setdefault('OPENCV_IO_ENABLE_OPENEXR', '1')
import cv2

BASE = Path(__file__).resolve().parents[2]
FIXTURES = BASE/'tests/fixtures/tiff-layout'
OUTPUT = BASE/'qa-output/tiff-layout'
cases = json.loads((FIXTURES/'manifest.json').read_text())
assert len(cases) == 864
parser = argparse.ArgumentParser()
parser.add_argument('--inputs-only', action='store_true')
inputs_only = parser.parse_args().inputs_only
references = {}
for case in cases:
    name = case['file']
    data = (FIXTURES/name).read_bytes()
    assert hashlib.sha256(data).hexdigest() == case['sha256']
    bits, channels = case['bits'], case['channels']
    itemsize = bits//8
    kind = 'u4' if bits == 32 else f'f{itemsize}'
    with tifffile.TiffFile(FIXTURES/name) as file:
        page = file.pages[0]
        assert page.imagewidth == 17 and page.imagelength == 19
        assert page.is_tiled == case['tiled'] and file.is_bigtiff == case['big']
        assert page.bitspersample == bits and int(page.sampleformat) == (1 if bits == 32 else 3)
        assert int(page.predictor) == ((2 if bits == 32 else 3) if case['predicted'] else 1)
        assert int(page.planarconfig) == (2 if case['planar'] else 1)
        if channels in (2, 4): assert int(page.extrasamples[0]) == (1 if case['associated'] else 2)
        offsets, lengths = page.dataoffsets, page.databytecounts
        xmp, profile = page.tags[700].value, page.tags[34675].value
        if isinstance(xmp, str): xmp = xmp.encode()
    count = 1 if case['planar'] else channels
    chunkheight, chunkwidth = (16, 16) if case['tiled'] else (7, 17)
    planes = []
    chunk = 0
    for plane in range(channels if case['planar'] else 1):
        image = np.empty((19, 17, count), dtype=kind)
        for y in range(0, 19, chunkheight):
            for x in range(0, 17, chunkwidth):
                raw = zlib.decompress(data[offsets[chunk]:offsets[chunk]+lengths[chunk]])
                rows = len(raw)//(chunkwidth*count*itemsize)
                assert rows == (16 if case['tiled'] else min(7, 19-y))
                if case['predicted'] and bits != 32:
                    # Sum each channel's differenced bytes over the entire shuffled row.
                    encoded = np.frombuffer(raw, 'u1').reshape(rows, -1, count)
                    accumulated = np.cumsum(encoded, axis=1, dtype='u8').astype('u1').reshape(rows, itemsize, chunkwidth*count)
                    raw = accumulated.transpose(0, 2, 1).copy().tobytes()
                    values = np.frombuffer(raw, '>'+kind).reshape(rows, chunkwidth, count)
                else:
                    values = np.frombuffer(raw, case['endian']+kind).reshape(rows, chunkwidth, count)
                    if case['predicted']:
                        values = np.cumsum(values, axis=1, dtype='u8').astype('u4')
                actualheight, actualwidth = min(rows, 19-y), min(chunkwidth, 17-x)
                image[y:y+actualheight, x:x+actualwidth] = values[:actualheight, :actualwidth]
                chunk += 1
        planes.append(image)
    assert chunk == len(offsets)
    stored = np.concatenate(planes, axis=2).astype('f8')
    key = (bits, channels, case['white'], case['associated'])
    if key not in references:
        assert not case['predicted'] and not case['tiled'] and not case['planar']
        with tifffile.TiffFile(FIXTURES/name) as file:
            baseline = file.asarray().astype('f8')
        if channels == 1: baseline = baseline[:, :, None]
        np.testing.assert_array_equal(stored.view('u8'), baseline.view('u8'))
        references[key] = stored.copy()
    else:
        np.testing.assert_array_equal(stored.view('u8'), references[key].view('u8'), err_msg=name)
    if inputs_only: continue
    if bits == 32: stored /= 4294967295.
    alpha = stored[:, :, -1] if channels in (2, 4) else np.ones((19, 17))
    native = stored[:, :, :1 if channels < 3 else 3]
    if case['associated']:
        numerator = alpha[:, :, None]-native if case['white'] else native
        native = np.divide(numerator, alpha[:, :, None], out=np.zeros_like(native), where=alpha[:, :, None] != 0)
    elif case['white']:
        native = 1.-native
    if channels < 3: native = np.repeat(native, 3, axis=2)
    expected = np.concatenate([native.astype('f4'), alpha[:, :, None].astype('f4')], axis=2)
    if case['orientation'] in (6, 8): expected = np.rot90(expected, -1 if case['orientation'] == 6 else 1)
    actual = np.fromfile(OUTPUT/(name+'.f32'), '<f4').reshape(expected.shape)
    np.testing.assert_array_equal(actual.view('u4'), expected.copy().view('u4'), err_msg=name)
    expected_composite = expected.copy()
    expected_composite[expected[:, :, 3] == 0] = 0
    composite = np.fromfile(OUTPUT/(name+'.comp.f32'), '<f4').reshape(expected.shape)
    np.testing.assert_array_equal(composite.view('u4'), expected_composite.view('u4'), err_msg=name)
    with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        manifest = json.loads(project.read('manifest.json'))
        assert manifest['version'] == 13 and manifest['layers'][0]['bit_depth'] == 32
        assert project.read('metadata/source.xmp') == xmp
        assert project.read('metadata/source.icc') == profile
        decoded = cv2.imdecode(np.frombuffer(project.read(f"layers/{manifest['layers'][0]['id']}.exr"), dtype='u1'), cv2.IMREAD_UNCHANGED)
        assert decoded is not None
        np.testing.assert_array_equal(decoded[:, :, [2, 1, 0, 3]].copy().view('u4'), actual.view('u4'))
    with tifffile.TiffFile(OUTPUT/(name+'-working.tiff')) as file:
        np.testing.assert_array_equal(file.asarray().view('u4'), composite.view('u4'))
    decoded = cv2.imread(str(OUTPUT/(name+'-working.exr')), cv2.IMREAD_UNCHANGED)
    assert decoded is not None
    np.testing.assert_array_equal(decoded[:, :, [2, 1, 0, 3]].copy().view('u4'), composite.view('u4'))
assert len(references) == 27
print('TIFF chunk layouts: 864 pinned inputs/279072 pixels, float16/64+UINT32, predictors3/2+off, 19x17 partial tiles and multirow strips, planar/classic+BigTIFF/both endian/gray+RGB/alpha+white; independent zlib/cumsum/chunk crop +27 tifffile native baselines; '+('inputs verified' if inputs_only else 'exact f32-rounded color+alpha/project metadata+CRC/OpenCV EXR/TIFF bits verified'))
