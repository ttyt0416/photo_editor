"""Independent TIFF chunks: NumPy cumsum predictions, edge cropping and outputs."""
from pathlib import Path
import hashlib
import json
import os
import zipfile
import zlib
import argparse
import struct
import numpy as np
import tifffile
from icc_reference import tags, curve, matrix, encode_srgb
os.environ.setdefault('OPENCV_IO_ENABLE_OPENEXR', '1')
import cv2

BASE = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--inputs-only', action='store_true')
parser.add_argument('--planar-float32', action='store_true')
parser.add_argument('--planar-integer', action='store_true')
args = parser.parse_args()
inputs_only, planar_float32 = args.inputs_only, args.planar_float32
planar_integer = args.planar_integer
assert not (planar_float32 and planar_integer)
folder = 'tiff-rgb-planar-integer' if planar_integer else 'tiff-rgb-planar-float' if planar_float32 else 'tiff-layout'
FIXTURES = BASE/'tests/fixtures'/folder
OUTPUT = BASE/'qa-output'/folder
cases = json.loads((FIXTURES/'manifest.json').read_text())
assert len(cases) == (192 if planar_integer else 144 if planar_float32 else 864)
destination = matrix(tags((BASE/'data/color/srgb.icc').read_bytes()))
references = {}
maximum = 0
for case in cases:
    case.setdefault('bits', 32)
    case.setdefault('white', False)
    case.setdefault('associated', False)
    case.setdefault('planar', True)
    name = case['file']
    data = (FIXTURES/name).read_bytes()
    assert hashlib.sha256(data).hexdigest() == case['sha256']
    bits, channels = case['bits'], case['channels']
    itemsize = bits//8
    kind = f'u{itemsize}' if planar_integer else 'u4' if bits == 32 and not planar_float32 else f'f{itemsize}'
    with tifffile.TiffFile(FIXTURES/name) as file:
        page = file.pages[0]
        assert page.imagewidth == 17 and page.imagelength == 19
        assert page.is_tiled == case['tiled'] and file.is_bigtiff == case['big']
        assert page.bitspersample == bits and int(page.sampleformat) == (1 if kind.startswith('u') else 3)
        assert int(page.predictor) == ((2 if kind.startswith('u') else 3) if case['predicted'] else 1)
        assert int(page.planarconfig) == (2 if case['planar'] else 1)
        if channels in (2, 4): assert int(page.extrasamples[0]) == (1 if case['associated'] else 2)
        offsets, lengths = page.dataoffsets, page.databytecounts
        xmp = page.tags[700].value
        profile = page.tags[34675].value if 34675 in page.tags else None
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
                if case['predicted'] and not kind.startswith('u'):
                    # Sum each channel's differenced bytes over the entire shuffled row.
                    encoded = np.frombuffer(raw, 'u1').reshape(rows, -1, count)
                    accumulated = np.cumsum(encoded, axis=1, dtype='u8').astype('u1').reshape(rows, itemsize, chunkwidth*count)
                    raw = accumulated.transpose(0, 2, 1).copy().tobytes()
                    values = np.frombuffer(raw, '>'+kind).reshape(rows, chunkwidth, count)
                else:
                    values = np.frombuffer(raw, case['endian']+kind).reshape(rows, chunkwidth, count)
                    if case['predicted']:
                        values = np.cumsum(values, axis=1, dtype='u8').astype(kind)
                actualheight, actualwidth = min(rows, 19-y), min(chunkwidth, 17-x)
                image[y:y+actualheight, x:x+actualwidth] = values[:actualheight, :actualwidth]
                chunk += 1
        planes.append(image)
    assert chunk == len(offsets)
    stored = np.concatenate(planes, axis=2).astype('f8')
    key = (bits, channels, case['white'], case['associated'])
    if key not in references:
        assert not case['predicted'] and not case['tiled'] and (planar_integer or planar_float32 or not case['planar'])
        with tifffile.TiffFile(FIXTURES/name) as file:
            baseline = file.asarray().astype('f8')
        if channels == 1: baseline = baseline[:, :, None]
        elif case['planar']: baseline = np.moveaxis(baseline, 0, 2)
        np.testing.assert_array_equal(stored.view('u8'), baseline.view('u8'))
        references[key] = stored.copy()
    else:
        np.testing.assert_array_equal(stored.view('u8'), references[key].view('u8'), err_msg=name)
    if inputs_only: continue
    if planar_integer:
        maximum_integer = (1<<bits)-1
        rgba = np.concatenate([stored[:,:,:3], stored[:,:,3:4] if channels==4 else np.full((19,17,1), maximum_integer)],axis=2).astype(kind)
        if case['profile'] == 'matrix':
            parsed = tags(profile)
            decoded = np.stack([curve(parsed[k],rgba[:,:,i].astype('f8')/maximum_integer) for i,k in enumerate(['rTRC','gTRC','bTRC'])],axis=2)
            linear = decoded @ (np.linalg.inv(destination) @ matrix(parsed)).T
            target = tags((BASE/'data/color/srgb.icc').read_bytes())
            g,a,b,c,d = np.array(struct.unpack_from('>5i',target['rTRC'],12))/65536
            expected = np.where(linear<c*d, linear/c, (np.maximum(linear,0)**(1/g)-b)/a)
            rgba[:,:,:3] = np.rint(np.clip(expected,0,1)*maximum_integer).astype(kind)
        if case['orientation'] in (6,8): rgba = np.rot90(rgba,-1 if case['orientation']==6 else 1)
        actual = np.fromfile(OUTPUT/(name+'.rgba'), '<'+kind).reshape(rgba.shape)
        error = np.abs(actual[:,:,:3].astype('i4')-rgba[:,:,:3].astype('i4')).max()
        assert error <= 1, (name,error)
        maximum = max(maximum,error)
        np.testing.assert_array_equal(actual[:,:,3],rgba[:,:,3])
        if case['profile'] != 'matrix': np.testing.assert_array_equal(actual,rgba)
        with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
            assert project.testzip() is None
            manifest = json.loads(project.read('manifest.json'))
            assert manifest['version'] == 13 and manifest['layers'][0]['bit_depth'] == bits
            assert project.read('metadata/source.xmp') == xmp
            if profile is not None: assert project.read('metadata/source.icc') == profile
            decoded = cv2.imdecode(np.frombuffer(project.read(f"layers/{manifest['layers'][0]['id']}.png"),dtype='u1'),cv2.IMREAD_UNCHANGED)
            np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]],actual)
        composite = actual.copy()
        composite[actual[:,:,3] == 0] = 0
        with tifffile.TiffFile(OUTPUT/(name+'-working.tiff')) as file:
            assert file.pages[0].tags[34675].value == (BASE/'data/color/srgb.icc').read_bytes()
            np.testing.assert_array_equal(file.asarray(),composite)
        continue
    if kind == 'u4': stored /= 4294967295.
    alpha = stored[:, :, -1] if channels in (2, 4) else np.ones((19, 17))
    native = stored[:, :, :1 if channels < 3 else 3]
    if case['associated']:
        numerator = alpha[:, :, None]-native if case['white'] else native
        native = np.divide(numerator, alpha[:, :, None], out=np.zeros_like(native), where=alpha[:, :, None] != 0)
    elif case['white']:
        native = 1.-native
    if channels < 3: native = np.repeat(native, 3, axis=2)
    native = native.astype('f4')
    if case.get('profile') == 'matrix':
        parsed = tags(profile)
        decoded = []
        for c, key in enumerate(('rTRC','gTRC','bTRC')):
            tag = parsed[key]
            assert tag[:4] == b'para' and struct.unpack_from('>H',tag,8)[0] == 0
            gamma = struct.unpack_from('>i',tag,12)[0]/65536
            decoded.append(np.sign(native[:,:,c])*np.abs(native[:,:,c].astype('f8'))**gamma)
        native = encode_srgb(np.stack(decoded,axis=2)@(np.linalg.inv(destination)@matrix(parsed)).T)
    expected = np.concatenate([native.astype('f4'), alpha[:, :, None].astype('f4')], axis=2)
    if case['orientation'] in (6, 8): expected = np.rot90(expected, -1 if case['orientation'] == 6 else 1)
    actual = np.fromfile(OUTPUT/(name+'.f32'), '<f4').reshape(expected.shape)
    error = np.max(np.abs(actual[:,:,:3]-expected[:,:,:3])/np.maximum(1,np.abs(expected[:,:,:3])))
    assert error < .0008, (name,error)
    maximum = max(maximum,error)
    np.testing.assert_array_equal(actual[:,:,3].view('u4'),expected[:,:,3].copy().view('u4'))
    if case.get('profile') != 'matrix':
        np.testing.assert_array_equal(actual.view('u4'), expected.copy().view('u4'), err_msg=name)
    expected_composite = actual.copy()
    expected_composite[expected[:, :, 3] == 0] = 0
    composite = np.fromfile(OUTPUT/(name+'.comp.f32'), '<f4').reshape(expected.shape)
    np.testing.assert_array_equal(composite.view('u4'), expected_composite.view('u4'), err_msg=name)
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
    decoded = cv2.imread(str(OUTPUT/(name+'-working.exr')), cv2.IMREAD_UNCHANGED)
    assert decoded is not None
    np.testing.assert_array_equal(decoded[:, :, [2, 1, 0, 3]].copy().view('u4'), composite.view('u4'))
assert len(references) == (4 if planar_integer else 3 if planar_float32 else 27)
print(f'TIFF chunk layouts: {len(cases)} pinned inputs/{len(cases)*323} pixels, '+('UINT8/16 RGB planar native depth/ICC/alpha' if planar_integer else 'float32 RGB planar ICC/alpha' if planar_float32 else 'float16/64+UINT32/gray+RGB/alpha+white')+f', prediction+off/19x17 partial tiles+multirow strips/both endian/classic+BigTIFF; independent zlib/cumsum/chunk crop +{len(references)} tifffile native baselines; '+('inputs verified' if inputs_only else f'ICC error {maximum:g} <=1 integer unit; exact alpha/project PNG/source ICC+XMP+CRC/depth/working TIFF' if planar_integer else f'ICC error {maximum:.8g}<0.0008; exact identity/alpha/project metadata+CRC/OpenCV EXR/TIFF bits verified'))
