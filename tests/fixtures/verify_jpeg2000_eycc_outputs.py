"""QA-only C decoding and independent vector/scalar eYCC display math.
Matrix/rounding reference: OpenJPEG src/bin/common/color.c color_esycc_to_rgb.
Fixtures are generated locally; this does not certify the whole JP2 standard.
"""
from pathlib import Path
import json, os, struct, zipfile
os.environ['OPENCV_LOG_LEVEL'] = 'SILENT'
import numpy as np, cv2
from jpeg2000_c_reference import components, version
OUT = Path(__file__).resolve().parents[2] / 'qa-output/jpeg2000-input/eycc'
cases = json.loads((OUT/'manifest.json').read_text()); assert len(cases) == 60
pixels = 0
for case in cases:
    path = OUT / case['file']; planes = components(path)
    precision = case['precisions'][0]; limit = (1 << precision) - 1
    target = (1 << case['depth']) - 1; signed = case['signed']
    np.testing.assert_array_equal(planes.reshape(-1, planes.shape[2]).T, case['source_planes'])
    y = planes[:, :, 0].astype('f4')
    cb = planes[:, :, 1].astype('f4') - np.float32(0 if signed[1] else 1 << (precision - 1))
    cr = planes[:, :, 2].astype('f4') - np.float32(0 if signed[2] else 1 << (precision - 1))
    # Explicit f32 operations match the primary decoder display convention.
    r = y - np.float32(.0000368)*cb + np.float32(1.40199)*cr + np.float32(.5)
    g = np.float32(1.0003)*y - np.float32(.344125)*cb - np.float32(.7141128)*cr + np.float32(.5)
    b = np.float32(.999823)*y + np.float32(1.77204)*cb - np.float32(.000008)*cr + np.float32(.5)
    rgb = np.clip(np.trunc(np.stack([r, g, b], axis=2)), 0, limit).astype('i8') * target // limit
    if len(case['precisions']) == 4:
        ap = case['precisions'][3]; alimit = (1 << ap) - 1
        alpha = np.clip(planes[:, :, 3].astype('i8') + ((1 << (ap - 1)) if signed[3] else 0), 0, alimit) * target // alimit
    else:
        alpha = np.full(y.shape, target, dtype='i8')
    rgba = np.concatenate([rgb, alpha[:, :, None]], axis=2)
    actual = np.fromfile(OUT / case['decoded'], dtype='<f4').reshape(5, 7, 4)
    np.testing.assert_allclose(actual, rgba.astype('f4') / np.float32(target), rtol=0, atol=6e-8, err_msg=case['file'])
    encoded = path.read_bytes(); at = encoded.index(b'colr')
    assert struct.unpack_from('>I', encoded, at+7)[0] == 24
    if len(case['precisions']) == 4:
        at = encoded.index(b'cdef')+4
        assert struct.unpack_from('>H', encoded, at)[0] == 4
        assert struct.unpack_from('>HHH', encoded, at+2+3*6) == (3, 1, 0)
    display = rgba.copy(); display[:, :, :3][alpha == 0] = 0
    png = cv2.imdecode(np.frombuffer((OUT/(case['file']+'.png')).read_bytes(), 'u1'), cv2.IMREAD_UNCHANGED)
    np.testing.assert_array_equal(png[:, :, [2,1,0,3]], display.astype('u1' if case['depth']==8 else 'u2'))
    with zipfile.ZipFile(OUT/(case['file']+'.rphoto')) as archive:
        assert archive.testzip() is None
        doc = json.loads(archive.read('manifest.json'))
        assert doc['width'] == 7 and doc['height'] == 5
        assert doc['layers'][0]['bit_depth'] == case['depth']
    pixels += y.size
print(f'eYCC JPEG2000: C OpenJPEG {version}, 60 JP2/{pixels} pixels; exact original components, all four signed/unsigned chroma combinations, 1/4/8/12/16-bit color, mixed unsigned8/signed16 alpha, COLR24/CDEF, independent f32 matrix and exact PNG; float error <=6e-8; project ZIP/CRC/depth.')
