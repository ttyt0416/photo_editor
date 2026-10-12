"""Independent gray-alpha TIFF planes/unassociation/ICC and float asset bytes."""
from pathlib import Path
import hashlib
import json
import zipfile
import os
import numpy as np
import tifffile
from icc_reference import encode_srgb
os.environ.setdefault('OPENCV_IO_ENABLE_OPENEXR','1')
import cv2

BASE=Path(__file__).resolve().parents[2]
FIXTURES=BASE/'tests/fixtures/tiff-gray-alpha'
OUTPUT=BASE/'qa-output/tiff-gray-alpha'
cases=json.loads((FIXTURES/'manifest.json').read_text())
assert len(cases)==96
maximum=0
for case in cases:
    name=case['file']
    path=FIXTURES/name
    assert hashlib.sha256(path.read_bytes()).hexdigest()==case['sha256']
    with tifffile.TiffFile(path) as file:
        page=file.pages[0]
        stored=page.asarray().astype('f4')
        if case['planar']: stored=np.moveaxis(stored,0,2)
        assert stored.shape==(3,17,2)
        assert int(page.extrasamples[0])==(1 if case['associated'] else 2)
        xmp=page.tags[700].value
        if isinstance(xmp,str):xmp=xmp.encode()
        profile=None if case['profile']=='none' else page.tags[34675].value
    alpha=stored[:,:,1]
    if case['associated']:
        gray=np.zeros(alpha.shape,dtype='f4')
        np.divide(stored[:,:,0].astype('f8'),alpha.astype('f8'),out=gray,where=alpha!=0,casting='unsafe')
    else:gray=stored[:,:,0]
    expected=encode_srgb(gray.astype('f8')) if case['profile']=='linear' else gray
    if case['orientation']==6:expected=np.rot90(expected,-1);alpha=np.rot90(alpha,-1)
    if case['orientation']==8:expected=np.rot90(expected,1);alpha=np.rot90(alpha,1)
    actual=np.fromfile(OUTPUT/(name+'.f32'),'<f4').reshape(*expected.shape,4)
    composite=np.fromfile(OUTPUT/(name+'.comp.f32'),'<f4').reshape(*expected.shape,4)
    error=np.max(np.abs(actual[:,:,0]-expected)/np.maximum(1,np.abs(expected)))
    assert error<.0008,(name,error)
    maximum=max(maximum,error)
    for c in [1,2]:np.testing.assert_array_equal(actual[:,:,c].view('u4'),actual[:,:,0].view('u4'))
    np.testing.assert_array_equal(actual[:,:,3].view('u4'),alpha.copy().view('u4'))
    if case['profile']!='linear':np.testing.assert_array_equal(actual[:,:,0].view('u4'),expected.copy().view('u4'))
    expected_composite=actual.copy();expected_composite[alpha==0]=0
    np.testing.assert_array_equal(composite.view('u4'),expected_composite.view('u4'))
    with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        assert project.read('metadata/source.xmp')==xmp
        manifest=json.loads(project.read('manifest.json'))
        assert manifest['version']==13 and manifest['layers'][0]['bit_depth']==32
        layer=manifest['layers'][0]['id']
        decoded=cv2.imdecode(np.frombuffer(project.read(f'layers/{layer}.exr'),dtype='u1'),cv2.IMREAD_UNCHANGED)
        assert decoded is not None
        np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]].copy().view('u4'),actual.view('u4'))
        if profile is not None:assert project.read('metadata/source.icc')==profile
    with tifffile.TiffFile(OUTPUT/(name+'-working.tiff')) as file:
        np.testing.assert_array_equal(file.asarray().view('u4'),composite.view('u4'))
    decoded=cv2.imread(str(OUTPUT/(name+'-working.exr')),cv2.IMREAD_UNCHANGED)
    assert decoded is not None
    np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]].copy().view('u4'),composite.view('u4'))
print(f'Gray float alpha TIFF: 96 pinned inputs/4896 pixels, associated+straight/contiguous+separate/classic+BigTIFF/both endian/raw+Deflate/orientations; independent gray/unassociation/sRGB error {maximum:.8g}<0.0008; exact signed alpha/identity hidden RGB/project assets+CRC/working TIFF floats and independent OpenCV project/output EXR bits; alpha0 associated RGB becomes0, straight hidden RGB remains in layer.')
