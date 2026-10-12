"""Independent ICC matrix/TRC float conversion, alpha and preserved projects."""
from pathlib import Path
import json
import struct
import zipfile
import zlib
import numpy as np
from icc_reference import tags, matrix, encode_srgb

ROOT = Path(__file__).resolve().parents[2] / 'qa-output/jxl-color'
destination = matrix(tags((ROOT / 'working.icc').read_bytes()))


def curve(tag, x):
    if tag[:4] == b'curv':
        count = struct.unpack_from('>I', tag, 8)[0]
        if count == 0:
            return x
        if count == 1:
            return np.sign(x) * np.abs(x) ** (struct.unpack_from('>H', tag, 12)[0] / 256)
        values = np.frombuffer(tag[12:12+2*count], dtype='>u2').astype('f8') / 65535
        result = np.interp(x, np.linspace(0, 1, count), values)
        result[x < 0] = values[0] + x[x < 0] * (values[1] - values[0]) * (count - 1)
        result[x > 1] = values[-1] + (x[x > 1] - 1) * (values[-1] - values[-2]) * (count - 1)
        return result
    assert tag[:4] == b'para'
    kind = struct.unpack_from('>H', tag, 8)[0]
    params = np.array(struct.unpack_from('>'+'i'*[1,3,4,5,7][kind], tag, 12)) / 65536
    if kind == 0:
        return np.sign(x) * np.abs(x) ** params[0]
    assert kind in [3, 4]
    if kind == 3:
        params = np.concatenate([params, [0,0]])
    g,a,b,c,d,e,f = params
    return np.where(x >= d, np.maximum(a*x+b,0)**g + e, c*x + f)


def check(name, gray, w, h, depth):
    source = np.frombuffer(zlib.decompress((ROOT / (name+'.src.zlib')).read_bytes()), dtype='<f4').reshape(h,w,4)
    actual = np.frombuffer(zlib.decompress((ROOT / (name+'.dst.zlib')).read_bytes()), dtype='<f4').reshape(h,w,4)
    profile = tags((ROOT / (name+'.icc')).read_bytes())
    if gray:
        linear = np.repeat(curve(profile['kTRC'], source[:,:,:1].astype('f8')), 3, axis=2)
    else:
        decoded = np.stack([curve(profile[k], source[:,:,c].astype('f8')) for c,k in enumerate(['rTRC','gTRC','bTRC'])], axis=2)
        linear = decoded @ (np.linalg.inv(destination) @ matrix(profile)).T
    expected = encode_srgb(linear)
    if depth < 32:
        expected = np.round(np.clip(expected,0,1) * ((1 << depth)-1)) / ((1 << depth)-1)
    # Fixed 16.16 ICC parameters and the extended-range gamma classifier have
    # bounded relative error above SDR; use SDR units within [-1,1] and relative
    # units for larger HDR values, without allowing a large error near zero.
    error = np.max(np.abs(expected - actual[:,:,:3]) / np.maximum(1, np.abs(expected)))
    assert error < 0.0008, (name, error)
    np.testing.assert_array_equal(source[:,:,3].view('u4'),actual[:,:,3].view('u4'))
    return float(error)


cases = json.loads((ROOT / 'manifest.json').read_text())
assert len(cases) == 8
errors = []
pixels = 0
for case in cases:
    errors.append(check(case['name'], case['gray'], case['w'], case['h'], case['depth']))
    if case['name'] == 'grayscale_jpeg':
        data = np.frombuffer(zlib.decompress((ROOT / 'grayscale_jpeg.dst.zlib').read_bytes()), dtype='<f4').reshape(case['h'],case['w'],4)
        assert case['depth'] == 32 and data[:,:,:3].min() < 0 and data[:,:,:3].max() > 1
    with zipfile.ZipFile(ROOT / (case['name']+'.rphoto')) as project:
        assert project.testzip() is None
        if case['name'] in ['bench_oriented_brg','grayscale']:
            original = Path(__file__).resolve().parent / 'jxl-icc' / case['name'] / 'original.icc'
            assert project.read('metadata/source.icc') == original.read_bytes()
    pixels += case['w'] * case['h']
errors.append(check('synthetic',False,17,3,32))
values = np.frombuffer(zlib.decompress((ROOT / 'synthetic.dst.zlib').read_bytes()),dtype='<f4').reshape(3,17,4)
assert values[:,:,0].min() < 0 and values[:,:,1].max() > 1
print(f'JXL working sRGB: 8 pinned inputs/{pixels} pixels plus 51 synthetic HDR/negative/alpha samples, independent ICC matrix/TRC/sRGB max SDR-absolute/HDR-relative error {max(errors):.8g} <0.0008, alpha bits/project CRC/original ICC intact; EXR exact floats verified in Rust, no tone mapping.')
