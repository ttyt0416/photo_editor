"""Independent TIFF index packing, uint16 palettes/ICC and PNG16 project assets."""
from pathlib import Path
import hashlib
import json
import struct
import zipfile
import zlib
import numpy as np
import tifffile
import cv2
from icc_reference import tags, matrix, encode_srgb

BASE=Path(__file__).resolve().parents[2]
FIXTURES=BASE/'tests/fixtures/tiff-palette'
OUTPUT=BASE/'qa-output/tiff-palette'
cases=json.loads((FIXTURES/'manifest.json').read_text())
assert len(cases)==240
destination=matrix(tags((BASE/'data/color/srgb.icc').read_bytes()))
maximum=0
for case in cases:
    name=case['file']; data=(FIXTURES/name).read_bytes(); bits=case['bits']
    assert hashlib.sha256(data).hexdigest()==case['sha256']
    with tifffile.TiffFile(FIXTURES/name) as file:
        page=file.pages[0]
        assert page.photometric==3 and page.samplesperpixel==1 and page.bitspersample==bits
        assert page.is_tiled==case['tiled'] and file.is_bigtiff==case['big']
        colors=page.colormap
        assert colors.shape==(3,1<<bits) and colors.dtype==np.dtype('u2')
        offsets,lengths=page.dataoffsets,page.databytecounts
        xmp=page.tags[700].value
        if isinstance(xmp,str):xmp=xmp.encode()
        profile=None if case['profile']=='none' else page.tags[34675].value
    indices=np.empty((19,17),dtype='u2'); chunk=0
    chunkheight,chunkwidth=(16,16) if case['tiled'] else (7,17)
    for y in range(0,19,chunkheight):
        for x in range(0,17,chunkwidth):
            raw=data[offsets[chunk]:offsets[chunk]+lengths[chunk]]
            if case['compressed']:raw=zlib.decompress(raw)
            rows=16 if case['tiled'] else min(7,19-y)
            if bits<8:
                packed=np.frombuffer(raw,'u1').reshape(rows,(chunkwidth*bits+7)//8)
                unpacked=np.unpackbits(packed,axis=1,bitorder='big')[:,:chunkwidth*bits].reshape(rows,chunkwidth,bits)
                values=np.sum(unpacked*(1<<np.arange(bits-1,-1,-1)),axis=2).astype('u2')
            else:values=np.frombuffer(raw,'u1' if bits==8 else case['endian']+'u2').reshape(rows,chunkwidth)
            h,w=min(rows,19-y),min(chunkwidth,17-x)
            indices[y:y+h,x:x+w]=values[:h,:w];chunk+=1
    assert chunk==len(offsets)
    reference=((np.arange(323,dtype='u8').reshape(19,17)*1009+3)%(1<<bits)).astype('u2')
    reference[0,:2]=[0,(1<<bits)-1]
    np.testing.assert_array_equal(indices,reference)
    rgb=np.moveaxis(colors[:,indices],0,2)
    expected=rgb.astype('f8')/65535
    if case['profile']=='matrix':
        parsed=tags(profile); decoded=[]
        for c,key in enumerate(('rTRC','gTRC','bTRC')):
            tag=parsed[key]
            assert tag[:4]==b'para' and struct.unpack_from('>H',tag,8)[0]==0
            gamma=struct.unpack_from('>i',tag,12)[0]/65536
            decoded.append(expected[:,:,c]**gamma)
        expected=np.clip(encode_srgb(np.stack(decoded,axis=2)@(np.linalg.inv(destination)@matrix(parsed)).T),0,1)
    if case['orientation'] in (6,8):
        rotations=-1 if case['orientation']==6 else 1
        expected=np.rot90(expected,rotations);rgb=np.rot90(rgb,rotations)
    actual=np.fromfile(OUTPUT/(name+'.rgba16'),'<u2').reshape(*expected.shape[:2],4)
    assert np.all(actual[:,:,3]==65535)
    error=np.max(np.abs(actual[:,:,:3].astype('f8')/65535-expected))
    assert error<.0008,(name,error);maximum=max(maximum,error)
    if case['profile']!='matrix':np.testing.assert_array_equal(actual[:,:,:3],rgb)
    with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        manifest=json.loads(project.read('manifest.json'))
        assert manifest['version']==13 and manifest['layers'][0]['bit_depth']==16
        assert project.read('metadata/source.xmp')==xmp
        if profile is not None:assert project.read('metadata/source.icc')==profile
        decoded=cv2.imdecode(np.frombuffer(project.read(f"layers/{manifest['layers'][0]['id']}.png"),dtype='u1'),cv2.IMREAD_UNCHANGED)
        assert decoded is not None and decoded.dtype==np.dtype('u2')
        np.testing.assert_array_equal(decoded[:,:,[2,1,0,3]],actual)
    with tifffile.TiffFile(OUTPUT/(name+'-working.tiff')) as file:
        assert file.pages[0].bitspersample==16
        np.testing.assert_array_equal(file.asarray(),actual)
        assert file.pages[0].tags[34675].value==(BASE/'data/color/srgb.icc').read_bytes()
print(f'Palette TIFF: 240 pinned inputs/77520 pixels, 1/2/4/8/16-bit indices/full16-bit RGB maps/partial tiles+multirow strips/both endian/classic+BigTIFF/raw+Deflate/3ICC/orientation; independent bit arrays/index patterns/color-map/ICC math max {maximum:.8g}<0.0008; exact identity/project ICC+XMP+CRC/OpenCV PNG16/TIFF bits.')
