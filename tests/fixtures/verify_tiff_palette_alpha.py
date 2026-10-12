"""Independent palette index+alpha packing, RGB-table association and float assets."""
from pathlib import Path
import hashlib
import json
import os
import struct
import zipfile
import zlib
import numpy as np
import tifffile
from icc_reference import tags, matrix, encode_srgb
os.environ.setdefault('OPENCV_IO_ENABLE_OPENEXR','1')
import cv2

BASE=Path(__file__).resolve().parents[2]
FIXTURES=BASE/'tests/fixtures/tiff-palette-alpha'
OUTPUT=BASE/'qa-output/tiff-palette-alpha'
cases=json.loads((FIXTURES/'manifest.json').read_text())
assert len(cases)==320
destination=matrix(tags((BASE/'data/color/srgb.icc').read_bytes()))
maximum=0
for case in cases:
    name=case['file'];data=(FIXTURES/name).read_bytes();bits=case['bits'];count=1<<bits
    assert hashlib.sha256(data).hexdigest()==case['sha256']
    with tifffile.TiffFile(FIXTURES/name) as file:
        page=file.pages[0]
        assert page.photometric==3 and page.samplesperpixel==2 and page.bitspersample==bits
        assert int(page.extrasamples[0])==(1 if case['associated'] else 2)
        assert page.is_tiled==case['tiled'] and file.is_bigtiff==case['big']
        colors=page.colormap;assert colors.shape==(3,count)
        offsets,lengths=page.dataoffsets,page.databytecounts
        xmp=page.tags[700].value
        if isinstance(xmp,str):xmp=xmp.encode()
        profile=None if case['profile']=='none' else page.tags[34675].value
    components=1 if case['planar'] else 2
    chunkheight,chunkwidth=(16,16) if case['tiled'] else (7,17)
    planes=[];chunk=0
    for plane in range(2 if case['planar'] else 1):
        values=np.empty((19,17,components),dtype='u2')
        for y in range(0,19,chunkheight):
            for x in range(0,17,chunkwidth):
                raw=data[offsets[chunk]:offsets[chunk]+lengths[chunk]]
                if case['compressed']:raw=zlib.decompress(raw)
                rows=16 if case['tiled'] else min(7,19-y)
                if bits<8:
                    packed=np.frombuffer(raw,'u1').reshape(rows,(chunkwidth*components*bits+7)//8)
                    unpacked=np.unpackbits(packed,axis=1)[:,:chunkwidth*components*bits].reshape(rows,chunkwidth,components,bits)
                    block=np.sum(unpacked*(1<<np.arange(bits-1,-1,-1)),axis=3).astype('u2')
                else:block=np.frombuffer(raw,'u1' if bits==8 else case['endian']+'u2').reshape(rows,chunkwidth,components)
                h,w=min(rows,19-y),min(chunkwidth,17-x)
                values[y:y+h,x:x+w]=block[:h,:w];chunk+=1
        planes.append(values)
    assert chunk==len(offsets)
    stored=np.concatenate(planes,axis=2)
    indices=stored[:,:,0];alpha=stored[:,:,1].astype('f8')/(count-1)
    reference=((np.arange(323,dtype='u8').reshape(19,17)*1009+3)%count).astype('u2');reference[0,:2]=[0,count-1]
    np.testing.assert_array_equal(indices,reference)
    reference=((np.arange(323,dtype='u8').reshape(19,17)*31+7)%count).astype('u2');reference[0,:3]=[0,count-1,1]
    np.testing.assert_array_equal(stored[:,:,1],reference)
    native=np.moveaxis(colors[:,indices],0,2).astype('f8')/65535
    if case['associated']:
        native=np.divide(native,alpha[:,:,None],out=np.zeros_like(native),where=alpha[:,:,None]!=0)
    native=native.astype('f4')
    if case['profile']=='matrix':
        parsed=tags(profile);decoded=[]
        for c,key in enumerate(('rTRC','gTRC','bTRC')):
            tag=parsed[key];assert tag[:4]==b'para' and struct.unpack_from('>H',tag,8)[0]==0
            gamma=struct.unpack_from('>i',tag,12)[0]/65536
            decoded.append(np.sign(native[:,:,c])*np.abs(native[:,:,c].astype('f8'))**gamma)
        expected=encode_srgb(np.stack(decoded,axis=2)@(np.linalg.inv(destination)@matrix(parsed)).T)
    else:expected=native
    if case['orientation'] in (6,8):
        rotations=-1 if case['orientation']==6 else 1
        expected=np.rot90(expected,rotations);alpha=np.rot90(alpha,rotations)
    actual=np.fromfile(OUTPUT/(name+'.f32'),'<f4').reshape(*alpha.shape,4)
    composite=np.fromfile(OUTPUT/(name+'.comp.f32'),'<f4').reshape(*alpha.shape,4)
    error=np.max(np.abs(actual[:,:,:3]-expected)/np.maximum(1,np.abs(expected)))
    assert error<.0008,(name,error);maximum=max(maximum,error)
    np.testing.assert_array_equal(actual[:,:,3].view('u4'),alpha.astype('f4').view('u4'))
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
    decoded=cv2.imread(str(OUTPUT/(name+'-working.exr')),cv2.IMREAD_UNCHANGED)
    assert decoded is not None
    np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]].copy().view('u4'),composite.view('u4'))
print(f'Palette alpha TIFF: 320 pinned inputs/103360 pixels, 1/2/4/8/16-bit packed indices+alpha, associated+straight/planar/partial tiles+strips/classic+BigTIFF/both endian/raw+Deflate/cycling3ICC/orientations; independent index+alpha patterns/palette association+ICC error {maximum:.8g}<0.0008; exact identity/alpha/hidden colors/project ICC+XMP+CRC/OpenCV EXR/automatic TIFF bits.')
