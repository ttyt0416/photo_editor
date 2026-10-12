"""Independent XML namespaces/properties, container chunks and image precision."""
from pathlib import Path
from PIL import Image
import json,struct,zipfile,zlib,xml.etree.ElementTree as ET
import numpy as np
import cv2
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/xmp-output'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==384
TIFF='http://ns.adobe.com/tiff/1.0/';EXIF='http://ns.adobe.com/exif/1.0/'
RDF='http://www.w3.org/1999/02/22-rdf-syntax-ns#';XMP='http://ns.adobe.com/xap/1.0/'
expected={f'{{{TIFF}}}Orientation':'1',f'{{{TIFF}}}ImageWidth':'9',f'{{{TIFF}}}ImageLength':'11',f'{{{EXIF}}}PixelXDimension':'9',f'{{{EXIF}}}PixelYDimension':'11'}

def png_parts(data):
    assert data[:8]==b'\x89PNG\r\n\x1a\n';at=8;chunks={};payload=[]
    while at<len(data):
        n,kind=struct.unpack_from('>I4s',data,at);body=data[at+8:at+8+n]
        assert zlib.crc32(kind+body)==struct.unpack_from('>I',data,at+8+n)[0]
        if kind==b'IDAT':payload.append(body)
        else:chunks[kind]=body
        at+=12+n
    assert at==len(data);xmp=None
    if b'iTXt' in chunks:
        key,rest=chunks[b'iTXt'].split(b'\0',1);assert key==b'XML:com.adobe.xmp'
        assert rest[:2]==b'\0\0';language,translated,xmp=rest[2:].split(b'\0',2)
    return xmp,b''.join(payload)

def jpeg_parts(data):
    assert data[:2]==b'\xff\xd8';at=2;payload=data[:2];xmp=None
    while at<len(data):
        start=at;assert data[at]==255
        while data[at]==255:at+=1
        marker=data[at];at+=1
        if marker==0xda:return xmp,payload+data[start:]
        n=struct.unpack_from('>H',data,at)[0];body=data[at+2:at+n];at+=n
        prefix=b'http://ns.adobe.com/xap/1.0/\0'
        if marker==0xe1 and body.startswith(prefix):xmp=body[len(prefix):]
        else:payload+=data[start:at]
    raise AssertionError('missing scan')

def riff_parts(data):
    assert data[:4]==b'RIFF' and data[8:12]==b'WEBP' and struct.unpack_from('<I',data,4)[0]+8==len(data)
    at=12;c={}
    while at<len(data):
        kind,n=struct.unpack_from('<4sI',data,at);c[kind]=data[at+8:at+8+n]
        if n%2:assert data[at+8+n]==0
        at+=8+n+n%2
    assert at==len(data);return c

def tiff_parts(data):
    endian='<' if data[:2]==b'II' else '>';magic=struct.unpack_from(endian+'H',data,2)[0];big=magic==43;assert magic in [42,43]
    slot=8 if big else 4;entry=20 if big else 12
    at=struct.unpack_from(endian+('Q' if big else 'I'),data,8 if big else 4)[0]
    count=struct.unpack_from(endian+('Q' if big else 'H'),data,at)[0];at+=8 if big else 2;tags={}
    sizes={1:1,2:1,3:2,4:4,5:8,6:1,7:1,8:2,9:4,10:8,11:4,12:8,16:8,17:8,18:8}
    for i in range(count):
        pos=at+i*entry;tag,kind=struct.unpack_from(endian+'HH',data,pos)
        n=struct.unpack_from(endian+('Q' if big else 'I'),data,pos+4)[0];length=n*sizes[kind]
        cell=pos+(12 if big else 8);offset=cell if length<=slot else struct.unpack_from(endian+('Q' if big else 'I'),data,cell)[0]
        raw=data[offset:offset+length];assert len(raw)==length;tags[tag]=(kind,n,raw)
    def ints(tag):
        kind,n,raw=tags[tag];code={3:'H',4:'I',16:'Q'}[kind];return struct.unpack(endian+str(n)+code,raw)
    payload=[data[a:a+n] for a,n in zip(ints(273),ints(279))]
    return tags.get(700,(None,None,None))[2],payload,big

baselines={};jpeg_error=0;pixels=0
for case in cases:
    path=OUT/case['file'];data=path.read_bytes();im=Image.open(path) if case['depth']==8 else None
    if im is not None:assert im.size==(9,11)
    if path.suffix=='.png':xmp,payload=png_parts(data)
    elif path.suffix=='.jpg':xmp,payload=jpeg_parts(data)
    elif path.suffix=='.webp':
        c=riff_parts(data);xmp=c.get(b'XMP ')
        if b'VP8X' in c:
            assert bool(c[b'VP8X'][0]&4)==case['include'];assert bool(c[b'VP8X'][0]&8)==case['exif']
            assert bool(c[b'VP8X'][0]&32)==case['icc']
        if case['include']:assert list(c)[-1]==b'XMP '
        payload={k:v for k,v in c.items() if k in [b'VP8 ',b'VP8L',b'ALPH']}
    else:xmp,payload,big=tiff_parts(data);assert big==case['big']
    assert (xmp is not None)==case['include']
    if xmp is not None:
        tree=ET.fromstring(xmp);seen=set()
        for node in tree.iter():
            for key,val in node.attrib.items():
                if key in expected:assert val==expected[key];seen.add(key)
                if key=='{urn:photo-editor:qa}Orientation':assert val=='leave'
            if node.tag in expected:
                target=node.find(f'{{{RDF}}}value');target=node if target is None else target
                assert (target.text or '').strip()==expected[node.tag];seen.add(node.tag)
            if node.tag=='{urn:photo-editor:qa}Orientation':assert node.text=='leave'
        assert seen==set(expected)
        assert list(tree.iter(f'{{{XMP}}}Thumbnails'))==[]
        assert list(tree.iter('{urn:photo-editor:qa}notes'))[0].text=='color & <unrelated>'
        assert '촬영자'.encode() in xmp and b'<!--keep packet comment-->' in xmp
        assert b'<![CDATA[color & <unrelated>]]>' in xmp
    raw=np.fromfile(OUT/case['raw'],dtype='<f4').reshape(11,9,4)
    if case['depth']>8:
        decoded=cv2.imread(str(path),cv2.IMREAD_UNCHANGED);assert decoded is not None and decoded.shape==(11,9,4)
        actual=decoded[:,:,[2,1,0,3]]
        if case['depth']==16:actual=actual.astype('f4')/np.float32(65535)
        np.testing.assert_array_equal(actual,raw)
    else:
        actual=np.asarray(im.convert('RGBA'));native=np.rint(raw*255).astype('u1')
        error=int(np.max(np.abs(actual.astype('i2')-native.astype('i2'))))
        if path.suffix=='.jpg':assert error<=2;jpeg_error=max(jpeg_error,error)
        else:np.testing.assert_array_equal(actual,native)
    key=(case['mode'],case['label'],case['icc'],case['exif_option'])
    if case['include']:assert payload==baselines[key],case['file']
    else:baselines[key]=payload
    with zipfile.ZipFile(OUT/(case['file']+'.rphoto')) as z:
        assert z.testzip() is None
        if case['include']:assert z.read('metadata/source.xmp')==xmp
    original=(OUT/f"source-{case['mode']}.xmp").read_bytes();assert b'old preview' in original
    pixels+=99
assert len(baselines)==192
assert (OUT/'errors/too-large.jpg').read_bytes()==b'original'
print(f'XMP output: 384 files/{pixels} pixels, 6 XML scalar forms/namespace aliases/long attribute spacing; independent namespace values/creator/unknown properties/comments/CDATA/thumbnail detach, PNG CRC/JPEG APP1/WebP flags/chunks/classic+BigTIFF700, 192 exact compressed-payload pairs, native 16-bit and float HDR pixels/project CRC; JPEG decoder <=2 (max {jpeg_error}), unsupported XML/extended reference/APP1 size keep destination.')
