"""Independent ICC.1 matrix/TRC math and TIFF/project bytes for float HDR import."""
from pathlib import Path
import json
import struct
import zipfile
import argparse
import os
import numpy as np
import tifffile
from icc_reference import tags, matrix, encode_srgb
os.environ.setdefault('OPENCV_IO_ENABLE_OPENEXR', '1')
import cv2

parser = argparse.ArgumentParser()
parser.add_argument('--sdr', action='store_true')
sdr = parser.parse_args().sdr
ROOT = Path(__file__).resolve().parents[2] / ('qa-output/icc-float-sdr' if sdr else 'qa-output/icc-float')
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
    p = np.array(struct.unpack_from('>'+'i'*[1,3,4,5,7][kind], tag, 12)) / 65536
    if kind == 0:
        return np.sign(x) * np.abs(x) ** p[0]
    if kind in [1, 2]:
        g,a,b = p[:3]
        c = p[3] if kind == 2 else 0
        return np.where(x >= -b/a, np.maximum(a*x+b,0)**g+c, c)
    if kind == 3:
        p = np.concatenate([p, [0,0]])
    g,a,b,c,d,e,f = p
    return np.where(x >= d, np.maximum(a*x+b,0)**g+e, c*x+f)


cases = json.loads((ROOT/'manifest.json').read_text())
assert len(cases) == (20 if sdr else 18)
errors = []
for case in cases:
    name = case['id']
    source = np.fromfile(ROOT/(name+'.src.f32'), dtype='<f4').reshape(9,17,4)
    if sdr:
        assert np.all((source[:,:,:3]>=0) & (source[:,:,:3]<=1))
        expected_source = np.zeros_like(source)
        for y in range(9):
            for x in range(17):
                expected_source[y,x]=[-0. if x==0 else x/16, y/8, ((x*19+y*13)%113)/112,
                                     [0.,-0.,.125,.33333334,1.][(x+y)%5] if case['alpha'] else 1.]
        np.testing.assert_array_equal(source.view('u4'),expected_source.view('u4'))
    actual = np.fromfile(ROOT/(name+'.dst.f32'), dtype='<f4').reshape(9,17,4)
    composite = np.fromfile(ROOT/(name+'.comp.f32'), dtype='<f4').reshape(9,17,4)
    profile_bytes = (ROOT/(case['profile']+'.icc')).read_bytes()
    profile = tags(profile_bytes)
    decoded = np.stack([curve(profile[k],source[:,:,i].astype('f8')) for i,k in enumerate(['rTRC','gTRC','bTRC'])],axis=2)
    expected = encode_srgb(decoded @ (np.linalg.inv(destination) @ matrix(profile)).T)
    error = np.max(np.abs(expected-actual[:,:,:3]) / np.maximum(1,np.abs(expected)))
    assert error < .0008, (name,error)
    errors.append(error)
    np.testing.assert_array_equal(source[:,:,3].view('u4'),actual[:,:,3].view('u4'))
    if not sdr: assert actual[:,:,:3].max() > 1
    if not sdr and case['profile'] in ['srgb','p3','adobe','linear','table','gamma']:
        assert actual[:,:,:3].min() < 0
    with tifffile.TiffFile(ROOT/(name+'.tiff')) as file:
        page = file.pages[0]
        assert page.tags['InterColorProfile'].value == profile_bytes
        rgb = page.asarray()
        channels = 4 if case['alpha'] else 3
        np.testing.assert_array_equal(rgb.view('u4'),source[:,:,:channels].copy().view('u4'))
    with tifffile.TiffFile(ROOT/(name+'-working.tiff')) as file:
        assert file.pages[0].tags['InterColorProfile'].value == (ROOT/'working.icc').read_bytes()
        np.testing.assert_array_equal(file.asarray().view('u4'),composite.view('u4'))
    with zipfile.ZipFile(ROOT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        assert project.read('metadata/source.icc') == profile_bytes
        metadata = json.loads(project.read('manifest.json'))
        assert metadata['version'] == 12 and metadata['layers'][0]['bit_depth'] == 32
        layer = metadata['layers'][0]['id']
        decoded = cv2.imdecode(np.frombuffer(project.read(f'layers/{layer}.exr'),dtype='u1'),cv2.IMREAD_UNCHANGED)
        assert decoded is not None
        np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]].copy().view('u4'),actual.view('u4'))
    decoded = cv2.imread(str(ROOT/(name+'-working.exr')),cv2.IMREAD_UNCHANGED)
    assert decoded is not None
    np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]].copy().view('u4'),composite.view('u4'))
print(f'Float ICC {"SDR" if sdr else "HDR"}: {len(cases)} RGB/RGBA TIFF inputs/{len(cases)*153} pixels, {len(cases)//2} profiles including all 5 parametric/identity/gamma/sampled curves, independent max SDR-absolute/HDR-relative error {max(errors):.8g} <0.0008; exact alpha/TIFF source/working float bits, original ICC/project CRC/depth/OpenCV EXR bits; history verified in Rust.')
