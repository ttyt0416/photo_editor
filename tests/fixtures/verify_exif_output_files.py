"""Independent EXIF dimensions/orientation, unchanged codec data, and source bytes."""
from pathlib import Path
from PIL import Image,ImageOps
import struct,json,zipfile,zlib
import numpy as np
import cv2
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/exif-output'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==258

def png_parts(data):
    at=8;result={};payload=[];assert data[:8]==b'\x89PNG\r\n\x1a\n'
    while at<len(data):
        n,kind=struct.unpack_from('>I4s',data,at);body=data[at+8:at+8+n]
        assert zlib.crc32(kind+body)==struct.unpack_from('>I',data,at+8+n)[0]
        if kind==b'IDAT':payload.append(body)
        else:result[kind]=body
        at+=n+12
    assert at==len(data);return result,b''.join(payload)

def no_app1(data):
    assert data[:2]==b'\xff\xd8';at=2;out=data[:2]
    while at<len(data):
        start=at;assert data[at]==255
        while data[at]==255:at+=1
        marker=data[at];at+=1
        if marker==0xda:return out+data[start:]
        n=struct.unpack_from('>H',data,at)[0];at+=n
        if marker!=0xe1:out+=data[start:at]
    raise AssertionError('missing JPEG scan')

def riff_parts(data):
    assert data[:4]==b'RIFF' and data[8:12]==b'WEBP' and struct.unpack_from('<I',data,4)[0]+8==len(data)
    at=12;c={}
    while at<len(data):
        kind,n=struct.unpack_from('<4sI',data,at);c[kind]=data[at+8:at+8+n]
        if n%2:assert data[at+8+n]==0
        at+=8+n+n%2
    assert at==len(data);return c

baselines={};jpeg_error=0;pixels=0
for case in cases:
    path=OUT/case['file'];data=path.read_bytes();im=Image.open(path);assert im.size==(9,11)
    exif=im.info.get('exif');exif=exif[6:] if exif and exif.startswith(b'Exif\0\0') else exif
    assert bool(exif)==case['include']
    original=Image.open(OUT/case['input']).info['exif'];original=original[6:] if original.startswith(b'Exif\0\0') else original
    if case['include']:
        tags=im.getexif();assert tags[274]==1 and tags[256]==9 and tags[257]==11 and tags[271]=='CameraQA'
        child=tags.get_ifd(0x8769);assert child[0xa002]==9 and child[0xa003]==11
        assert exif[:2]==(b'II' if case['little'] else b'MM') and len(exif)==len(original)
        endian='<' if case['little'] else '>';assert struct.unpack_from(endian+'I',exif,70)[0]==0
        assert exif[104:]==original[104:],'opaque Make/thumbnail bytes changed'
    assert im.info.get('icc_profile')==((ROOT/'data/color/srgb.icc').read_bytes() if case['icc'] else None)
    if case.get('depth')==16:
        decoded=cv2.imread(str(path),cv2.IMREAD_UNCHANGED)[:,:,[2,1,0,3]]
        np.testing.assert_array_equal(decoded,cv2.imread(str(OUT/case['golden']),cv2.IMREAD_UNCHANGED)[:,:,[2,1,0,3]])
        native=np.fromfile(OUT/case['raw'],dtype='<f4').reshape(11,9,4)
        np.testing.assert_array_equal(native,decoded.astype('f4')/np.float32(65535))
    else:
        actual=np.asarray(im.convert('RGBA'));native=np.fromfile(OUT/case['raw'],dtype='u1').reshape(11,9,4)
        error=int(np.max(np.abs(actual.astype('i2')-native.astype('i2'))))
        if case['format']=='jpg':assert error<=2;jpeg_error=max(jpeg_error,error)
        else:np.testing.assert_array_equal(actual,native)
        np.testing.assert_array_equal(actual,np.asarray(ImageOps.exif_transpose(im).convert('RGBA')))
        if case['lossless'] and case['format']!='jpg':np.testing.assert_array_equal(actual,np.asarray(Image.open(OUT/case['golden']).convert('RGBA')))
    if path.suffix=='.png':c,payload=png_parts(data);assert (b'eXIf' in c)==case['include']
    elif path.suffix=='.jpg':payload=no_app1(data)
    else:
        c=riff_parts(data);assert (b'EXIF' in c)==case['include']
        if b'VP8X' in c:assert bool(c[b'VP8X'][0]&8)==case['include']
        if case['include']:assert list(c)[-1]==b'EXIF'
        payload={key:value for key,value in c.items() if key in [b'VP8 ',b'VP8L',b'ALPH']}
    key=(case['input'],case['format'],case['icc'],case.get('depth',8))
    if case['include']:assert payload==baselines[key],case['file']
    else:baselines[key]=payload
    with zipfile.ZipFile(OUT/(case['file']+'.rphoto')) as z:assert z.testzip() is None
    pixels+=99
for little in [False,True]:
    for orientation in range(1,9):
        data=(OUT/f'wide-le{str(little).lower()}-o{orientation}.exif').read_bytes();tags=Image.Exif();tags.load(data)
        assert tags[256]==70000 and tags[257]==80000 and tags[274]==1 and tags[271]=='CameraQA'
        child=tags.get_ifd(0x8769);assert child[0xa002]==70000 and child[0xa003]==80000
assert len(baselines)==129
assert (OUT/'errors/too-large.jpg').read_bytes()==b'original'
print(f'EXIF output: 258 PNG/JPEG/WebP files/{pixels} pixels; independent orientation1/9x11 root+ExifIFD sizes/Make/endian/detached thumbnail, opaque bytes preserved; 129 exact metadata on-off PNG IDAT/JPEG non-APP1/WebP payload pairs, exact independent PNG/WebP pixels and native PNG16; JPEG decoder difference <=2 (max {jpeg_error}), 16 wide-dimension SHORT-to-LONG cases/project CRC/source profiles/atomic size error.')
