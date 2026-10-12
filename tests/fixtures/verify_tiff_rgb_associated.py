"""Independent TIFF RGB unassociation, ICC math and project/output EXR bits."""
from pathlib import Path
import hashlib
import json
import os
import struct
import zipfile
import numpy as np
import tifffile
from icc_reference import tags,matrix,encode_srgb
os.environ.setdefault('OPENCV_IO_ENABLE_OPENEXR','1')
import cv2

BASE=Path(__file__).resolve().parents[2]
FIXTURES=BASE/'tests/fixtures/tiff-rgb-associated'
OUTPUT=BASE/'qa-output/tiff-rgb-associated'
cases=json.loads((FIXTURES/'manifest.json').read_text())
assert len(cases)==48
destination=matrix(tags((BASE/'data/color/srgb.icc').read_bytes()))
maximum=0
for case in cases:
    name=case['file'];path=FIXTURES/name
    assert hashlib.sha256(path.read_bytes()).hexdigest()==case['sha256']
    with tifffile.TiffFile(path) as file:
        page=file.pages[0]
        stored=page.asarray().astype('f4')
        if case['planar']:stored=np.moveaxis(stored,0,2)
        assert stored.shape==(3,17,4) and int(page.extrasamples[0])==1
        xmp=page.tags[700].value
        if isinstance(xmp,str):xmp=xmp.encode()
        profile=None if case['profile']=='none' else page.tags[34675].value
    alpha=stored[:,:,3]
    native=np.zeros((3,17,3),dtype='f4')
    np.divide(stored[:,:,:3].astype('f8'),alpha[:,:,None].astype('f8'),out=native,where=alpha[:,:,None]!=0,casting='unsafe')
    if case['profile']=='matrix':
        parsed=tags(profile)
        decoded=[]
        for c,k in enumerate(['rTRC','gTRC','bTRC']):
            curve=parsed[k]
            assert curve[:4]==b'para' and struct.unpack_from('>H',curve,8)[0]==0
            gamma=struct.unpack_from('>i',curve,12)[0]/65536
            decoded.append(np.sign(native[:,:,c])*np.abs(native[:,:,c].astype('f8'))**gamma)
        expected=encode_srgb(np.stack(decoded,axis=2)@(np.linalg.inv(destination)@matrix(parsed)).T)
    else:expected=native
    if case['orientation']==6:expected=np.rot90(expected,-1);alpha=np.rot90(alpha,-1)
    if case['orientation']==8:expected=np.rot90(expected,1);alpha=np.rot90(alpha,1)
    actual=np.fromfile(OUTPUT/(name+'.f32'),'<f4').reshape(*alpha.shape,4)
    composite=np.fromfile(OUTPUT/(name+'.comp.f32'),'<f4').reshape(*alpha.shape,4)
    error=np.max(np.abs(actual[:,:,:3]-expected)/np.maximum(1,np.abs(expected)))
    assert error<.0008,(name,error)
    maximum=max(maximum,error)
    np.testing.assert_array_equal(actual[:,:,3].view('u4'),alpha.copy().view('u4'))
    if case['profile']!='matrix':np.testing.assert_array_equal(actual[:,:,:3].copy().view('u4'),expected.copy().view('u4'))
    expected_composite=actual.copy();expected_composite[alpha==0]=0
    np.testing.assert_array_equal(composite.view('u4'),expected_composite.view('u4'))
    with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        manifest=json.loads(project.read('manifest.json'))
        assert manifest['version']==13 and manifest['layers'][0]['bit_depth']==32
        assert project.read('metadata/source.xmp')==xmp
        if profile is not None:assert project.read('metadata/source.icc')==profile
        decoded=cv2.imdecode(np.frombuffer(project.read(f"layers/{manifest['layers'][0]['id']}.exr"),dtype='u1'),cv2.IMREAD_UNCHANGED)
        assert decoded is not None
        np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]].copy().view('u4'),actual.view('u4'))
    with tifffile.TiffFile(OUTPUT/(name+'-working.tiff')) as file:
        np.testing.assert_array_equal(file.asarray().view('u4'),composite.view('u4'))
        assert file.pages[0].tags[34675].value==(BASE/'data/color/srgb.icc').read_bytes()
    decoded=cv2.imread(str(OUTPUT/(name+'-working.exr')),cv2.IMREAD_UNCHANGED)
    assert decoded is not None
    np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]].copy().view('u4'),composite.view('u4'))
print(f'RGB float associated TIFF: 48 pinned inputs/2448 pixels, contiguous+separate/classic+BigTIFF/both endian/raw+Deflate/orientations; independent unassociation/matrix/gamma/sRGB max error {maximum:.8g}<0.0008; exact signed alpha/identity RGB/project ICC+XMP+CRC/OpenCV project+output EXR/working TIFF float bits; generic uncorrected premultiplication reproduced in Rust.')
