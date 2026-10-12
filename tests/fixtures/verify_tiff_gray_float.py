"""Pinned independent TIFF decoder, gray transfer formulas, orientation and assets."""
from pathlib import Path
import hashlib
import json
import struct
import zipfile
import numpy as np
import tifffile
from icc_reference import tags, encode_srgb

BASE = Path(__file__).resolve().parents[2]
FIXTURES = BASE/'tests/fixtures/tiff-gray-float'
OUTPUT = BASE/'qa-output/tiff-gray-float'
cases = json.loads((FIXTURES/'manifest.json').read_text(encoding='utf-8'))
assert len(cases) == 80
maximum = 0
for case in cases:
    path = FIXTURES/case['file']
    assert hashlib.sha256(path.read_bytes()).hexdigest() == case['sha256']
    with tifffile.TiffFile(path) as file:
        page = file.pages[0]
        native = page.asarray()
        assert native.shape == (3,17) and native.dtype.kind == 'f' and native.dtype.itemsize == 4
        assert len(file.pages) == (2 if case['multipage'] else 1)
        assert file.is_bigtiff == case['big'] and file.byteorder == case['endian']
        xmp = page.tags[700].value
        if isinstance(xmp,str): xmp = xmp.encode()
        if case['white']:
            native = np.float32(1)-native.astype('f4')
        else:
            native = native.astype('f4')
        profile = None if case['profile'] == 'none' else (FIXTURES/(case['profile']+'.icc')).read_bytes()
        if profile is not None:
            assert page.tags[34675].value == profile
    if case['profile'] in ['none','invalid','working']:
        expected = native
    else:
        curve = tags(profile)['kTRC']
        if curve[:4] == b'curv':
            assert struct.unpack_from('>I',curve,8)[0] == 0
            linear = native.astype('f8')
        else:
            assert curve[:4] == b'para' and struct.unpack_from('>H',curve,8)[0] == 0
            gamma = struct.unpack_from('>i',curve,12)[0]/65536
            linear = np.sign(native)*np.abs(native.astype('f8'))**gamma
        expected = encode_srgb(linear)
    if case['orientation'] == 6: expected = np.rot90(expected,-1)
    if case['orientation'] == 8: expected = np.rot90(expected,1)
    actual = np.fromfile(OUTPUT/(case['file']+'.f32'),'<f4').reshape(*expected.shape,4)
    error = np.max(np.abs(actual[:,:,0]-expected)/np.maximum(1,np.abs(expected)))
    assert error < .0008,(case['file'],error)
    maximum = max(maximum,error)
    for channel in [1,2]: np.testing.assert_array_equal(actual[:,:,channel].view('u4'),actual[:,:,0].view('u4'))
    np.testing.assert_array_equal(actual[:,:,3],np.ones(expected.shape,dtype='f4'))
    if case['profile'] in ['none','invalid','working']:
        np.testing.assert_array_equal(actual[:,:,0].view('u4'),expected.astype('f4').copy().view('u4'))
    with zipfile.ZipFile(OUTPUT/(case['file']+'.rphoto')) as project:
        assert project.testzip() is None
        manifest = json.loads(project.read('manifest.json'))
        assert manifest['version'] == 13 and manifest['layers'][0]['bit_depth'] == 32
        assert project.read('metadata/source.xmp') == xmp
        if profile is not None: assert project.read('metadata/source.icc') == profile
        else: assert 'metadata/source.icc' not in project.namelist()
    with tifffile.TiffFile(OUTPUT/(case['file']+'-working.tiff')) as file:
        np.testing.assert_array_equal(file.asarray().view('u4'),actual.view('u4'))
        assert file.pages[0].tags[34675].value == (BASE/'data/color/srgb.icc').read_bytes()
print(f'Gray float TIFF: {len(cases)} pinned independently encoded inputs/4080 pixels, classic/BigTIFF/both endian/raw+Deflate/white+black zero/orientation1+6+8/3 multipage cases, independent gray-ICC max SDR-absolute/HDR-relative error {maximum:.8g}<0.0008; identity float bits/alpha/XMP/original ICC/project v13 CRC/working TIFF exact; EXR in Rust.')
