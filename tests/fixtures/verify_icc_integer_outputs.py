"""Independent serialized ICC matrix/TRC and native integer/project checks."""
from pathlib import Path
import io, json, struct, zipfile
import numpy as np
import cv2
import tifffile
from PIL import Image, ImageCms
from icc_reference import tags, curve, matrix
ROOT = Path(__file__).resolve().parents[2] / 'qa-output/icc-integer'
working = (ROOT/'working.icc').read_bytes()
target = tags(working)
g,a,b,c,d = np.array(struct.unpack_from('>5i', target['rTRC'], 12))/65536
cases = json.loads((ROOT/'manifest.json').read_text()); assert len(cases) == 32
max_error = 0; lcms_max = 0; lcms_offset_black = 0
for case in cases:
    name = case['id']; maximum = (1 << case['depth'])-1
    source = np.fromfile(ROOT/(name+'.src.f32'), '<f4').reshape(9,17,4)
    actual = np.fromfile(ROOT/(name+'.dst.f32'), '<f4').reshape(9,17,4)
    source_integer = np.rint(source.astype('f8')*maximum).astype('u2')
    actual_integer = np.rint(actual.astype('f8')*maximum).astype('u2')
    profile_bytes = (ROOT/(case['profile']+'.icc')).read_bytes()
    profile = tags(profile_bytes)
    if case['profile'] == 'working':
        expected_integer = source_integer[:,:,:3]
    else:
        if case['gray']:
            linear = np.repeat(curve(profile['kTRC'], source[:,:,:1].astype('f8')), 3, axis=2)
        else:
            values = np.stack([curve(profile[k], source[:,:,i].astype('f8')) for i,k in enumerate(['rTRC','gTRC','bTRC'])],axis=2)
            linear = values @ (np.linalg.inv(matrix(target)) @ matrix(profile)).T
        expected = np.where(linear < c*d, linear/c, (np.maximum(linear,0)**(1/g)-b)/a)
        expected_integer = np.rint(np.clip(expected,0,1)*maximum).astype('u2')
    error = np.abs(actual_integer[:,:,:3].astype('i4')-expected_integer.astype('i4')).max()
    assert error <= 1, (name, error); max_error = max(max_error,error)
    np.testing.assert_array_equal(actual_integer[:,:,3],source_integer[:,:,3])
    if case['depth'] == 8:
        mode = 'L' if case['gray'] else 'RGB'
        pixels = source_integer[:,:,0] if case['gray'] else source_integer[:,:,:3]
        transform = ImageCms.buildTransformFromOpenProfiles(ImageCms.ImageCmsProfile(io.BytesIO(profile_bytes)), ImageCms.ImageCmsProfile(io.BytesIO(working)),mode,'RGB',renderingIntent=1,flags=ImageCms.Flags.NOOPTIMIZE)
        reference = np.asarray(ImageCms.applyTransform(Image.fromarray(pixels.astype('u1')),transform)).astype('i4')
        difference = np.abs(actual_integer[:,:,:3].astype('i4')-reference)
        if case['profile'] == 'para2':
            # LittleCMS cmsgamma.c type 3 returns zero when a*x+b == 0,
            # while the serialized ICC type 2 equation adds c at the boundary.
            # Retain the strict independent tag equation for ALL pixels above.
            black = np.all(source[:,:,:3] == 0, axis=2)
            assert black.sum() == 1 and np.all(reference[black] == 0)
            assert np.all(actual_integer[:,:,:3][black] == 25)
            lcms_offset_black = max(lcms_offset_black, difference[black].max())
            difference = difference[~black]
        lcms_error = difference.max()
        assert lcms_error <= 1, (name,lcms_error); lcms_max = max(lcms_max,lcms_error)
    original = cv2.imread(str(ROOT/(name+'.png')),cv2.IMREAD_UNCHANGED)
    channels = 2 if case['gray'] and case['alpha'] else 1 if case['gray'] else 4 if case['alpha'] else 3
    # OpenCV expands gray-alpha to BGRA; compare gray and alpha explicitly.
    if channels == 1: np.testing.assert_array_equal(original, source_integer[:,:,0])
    elif case['gray']: np.testing.assert_array_equal(original[:,:,[2,3]], source_integer[:,:,[0,3]])
    else: np.testing.assert_array_equal(original[:,:,[2,1,0,3] if channels==4 else [2,1,0]],source_integer[:,:,:channels])
    with zipfile.ZipFile(ROOT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        assert project.read('metadata/source.icc') == profile_bytes
        metadata = json.loads(project.read('manifest.json'))
        assert metadata['layers'][0]['bit_depth'] == case['depth']
        layer = metadata['layers'][0]['id']
        decoded = cv2.imdecode(np.frombuffer(project.read(f'layers/{layer}.png'),dtype='u1'),cv2.IMREAD_UNCHANGED)
        np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]], actual_integer)
    with tifffile.TiffFile(ROOT/(name+'-working.tiff')) as file:
        assert file.pages[0].tags['InterColorProfile'].value == working
        output = file.asarray()
        assert output.dtype.itemsize*8 == case['depth']
        np.testing.assert_array_equal(output[:,:,3],source_integer[:,:,3])
print(f'Integer ICC: 32 RGB/gray 8/16-bit PNG cases/4896 pixels, 8 matrix/identity/sampled/parametric profiles; independent actual-working ICC max {max_error} unit and LittleCMS NOOPTIMIZE 16 cases non-boundary max {lcms_max} unit, explicit type-2 offset black difference {lcms_offset_black} units; alpha/native PNG/project CRC/source ICC/layer pixels/depth/working TIFF tag unchanged.')
