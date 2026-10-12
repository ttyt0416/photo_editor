"""Independent Pillow/libwebp pixels and WebP RIFF ICC/chunk order/payloads."""
from pathlib import Path
from PIL import Image
import json,struct,zipfile
import numpy as np
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'qa-output/webp-icc'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==32
profile=(ROOT/'data/color/srgb.icc').read_bytes()

def chunks(data):
    assert data[:4]==b'RIFF' and data[8:12]==b'WEBP'
    assert struct.unpack_from('<I',data,4)[0]+8==len(data)
    at=12;result={}
    while at<len(data):
        kind,n=struct.unpack_from('<4sI',data,at)
        assert kind not in result and at+8+n+(n%2)<=len(data)
        result[kind]=data[at+8:at+8+n]
        if n%2:assert data[at+8+n]==0
        at+=8+n+n%2
    assert at==len(data);return result

pixels=0;baselines={}
for case in cases:
    path=OUT/case['file'];c=chunks(path.read_bytes())
    im=Image.open(path);actual=np.asarray(im.convert('RGBA')).copy();actual[:,:,:3][actual[:,:,3]==0]=0
    raw=np.fromfile(OUT/case['raw'],dtype='u1').reshape(case['height'],case['width'],4)
    np.testing.assert_array_equal(actual,raw,err_msg=case['file'])
    source=np.asarray(Image.open(OUT/case['source']).convert('RGBA'))
    np.testing.assert_array_equal(actual[:,:,3],source[:,:,3])
    if case['lossless']:np.testing.assert_array_equal(actual,source)
    assert im.info.get('icc_profile')==(profile if case['include'] else None)
    payload={key:value for key,value in c.items() if key in [b'VP8 ',b'VP8L',b'ALPH']}
    assert (b'VP8L' in payload)==case['lossless']
    key=(case['width'],case['height'],case['alpha'],case['lossless'],case['quality'],case['method'])
    if case['include']:
        assert list(c)[:2]==[b'VP8X',b'ICCP'];assert c[b'ICCP']==profile
        extension=c[b'VP8X'];assert len(extension)==10 and extension[0]&0x20
        assert extension[1:4]==b'\0'*3
        assert int.from_bytes(extension[4:7],'little')+1==case['width']
        assert int.from_bytes(extension[7:10],'little')+1==case['height']
        if case['alpha']:assert extension[0]&0x10
        if b'ALPH' in c:assert list(c).index(b'ALPH')<list(c).index(b'VP8 ')
        before,decoded=baselines[key];assert payload==before,case['file']
        np.testing.assert_array_equal(actual,decoded)
    else:
        assert b'ICCP' not in c
        if b'VP8X' in c:assert not c[b'VP8X'][0]&0x20
        baselines[key]=(payload,actual)
    with zipfile.ZipFile(OUT/(case['file']+'.rphoto')) as z:assert z.testzip() is None
    pixels+=case['width']*case['height']
assert len(baselines)==16
print(f'WebP ICC output: 32 files/{pixels} pixels, independent Pillow/libwebp exact RGBA/alpha, 16 pairs identical VP8/VP8L/ALPH payloads and decoded pixels; correct RIFF size/padding/VP8X dimensions/ICCP order/profile, exact lossless and project CRC; lossy quality 0/90/100 and method 0/4/6, opaque/alpha/single-column.')
