"""Independent packed TIFF bit arrays, FillOrder, gray curves and assets."""
from pathlib import Path
import hashlib
import json
import struct
import zipfile
import zlib
import numpy as np
import tifffile
from PIL import Image
from icc_reference import tags,encode_srgb

BASE=Path(__file__).resolve().parents[2]
FIXTURES=BASE/'tests/fixtures/tiff-gray-packed'
OUTPUT=BASE/'qa-output/tiff-gray-packed'
cases=json.loads((FIXTURES/'manifest.json').read_text())
assert len(cases)==256
maximum=0
for case in cases:
    path=FIXTURES/case['file'];data=path.read_bytes()
    assert hashlib.sha256(data).hexdigest()==case['sha256']
    with tifffile.TiffFile(path) as file:
        page=file.pages[0]
        assert page.shape==(3,17) and page.bitspersample==case['bits']
        assert file.is_bigtiff==case['big'] and file.byteorder==case['endian']
        assert int(page.photometric)==(0 if case['white'] else 1)
        assert page.tags['FillOrder'].value==case['order']
        xmp=page.tags[700].value
        if isinstance(xmp,str):xmp=xmp.encode()
        profile=None if case['profile']=='none' else page.tags[34675].value
        decoded=b''
        for offset,length in zip(page.dataoffsets,page.databytecounts):
            block=data[offset:offset+length];decoded+=zlib.decompress(block) if case['compressed'] else block
    stride=(17*case['bits']+7)//8
    bits=np.unpackbits(np.frombuffer(decoded,'u1').reshape(3,stride),axis=1,bitorder='big' if case['order']==1 else 'little')[:,:17*case['bits']]
    samples=(bits.reshape(3,17,case['bits'])*(1<<np.arange(case['bits']-1,-1,-1))).sum(axis=2)
    raw=(np.arange(51).reshape(3,17)*7+3)%(1<<case['bits'])
    np.testing.assert_array_equal(samples,raw)
    if case['white']:samples=(1<<case['bits'])-1-samples
    expected=samples.astype('f8')/((1<<case['bits'])-1)
    if case['profile'] in ['linear','gamma18']:
        if case['profile']=='gamma18':
            curve=tags(profile)['kTRC'];assert curve[:4]==b'para' and struct.unpack_from('>H',curve,8)[0]==0
            gamma=struct.unpack_from('>i',curve,12)[0]/65536
            expected=expected**gamma
        expected=encode_srgb(expected)
    expected=np.floor(np.clip(expected,0,1)*255+.5).astype('u1')
    if case['orientation']==6:expected=np.rot90(expected,-1)
    if case['orientation']==8:expected=np.rot90(expected,1)
    actual=np.fromfile(OUTPUT/(case['file']+'.rgba8'),'u1').reshape(*expected.shape,4)
    error=np.max(np.abs(actual[:,:,0].astype('i4')-expected.astype('i4')))
    assert error<=1,(case['file'],error)
    maximum=max(maximum,error)
    for c in [1,2]:np.testing.assert_array_equal(actual[:,:,c],actual[:,:,0])
    np.testing.assert_array_equal(actual[:,:,3],np.full(expected.shape,255,dtype='u1'))
    if case['profile'] in ['none','working']:np.testing.assert_array_equal(actual[:,:,0],expected)
    with zipfile.ZipFile(OUTPUT/(case['file']+'.rphoto')) as project:
        assert project.testzip() is None
        assert project.read('metadata/source.xmp')==xmp
        if profile is not None:assert project.read('metadata/source.icc')==profile
        manifest=json.loads(project.read('manifest.json'))
        assert manifest['version']==13 and manifest['layers'][0]['bit_depth']==8
    np.testing.assert_array_equal(np.asarray(Image.open(OUTPUT/(case['file']+'-working.png')).convert('RGBA')),actual)
print(f'Packed gray TIFF: 256 pinned inputs/13056 pixels, 2/4 bits/classic+BigTIFF/both endian/white+black zero/MSB+LSB/raw+Deflate/17-pixel padded rows/orientations; independent bit arrays/gray transfer math max {maximum} 8-bit unit <=1, identity exact/alpha255/ICC+XMP/project v13 CRC/PNG verified.')
