"""Independent encoded EXIF/XMP, original ICC, project bytes and orientation pixels."""
from pathlib import Path
from PIL import Image,ImageOps
import struct,json,zipfile,zlib
import numpy as np
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/source-metadata'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==49

def png_chunks(data):
    assert data[:8]==b'\x89PNG\r\n\x1a\n';at=8;result={}
    while at<len(data):
        n,kind=struct.unpack_from('>I4s',data,at);body=data[at+8:at+8+n]
        assert zlib.crc32(kind+body)==struct.unpack_from('>I',data,at+8+n)[0]
        result[kind]=body;at+=n+12
    assert at==len(data);return result

def jpeg_metadata(data):
    assert data[:2]==b'\xff\xd8';at=2;exif=xmp=None
    while at<len(data):
        assert data[at]==255
        while data[at]==255:at+=1
        marker=data[at];at+=1
        if marker==0xda:break
        n=struct.unpack_from('>H',data,at)[0];assert n>=2;body=data[at+2:at+n];at+=n
        if marker==0xe1 and body.startswith(b'Exif\0\0'):exif=body[6:]
        prefix=b'http://ns.adobe.com/xap/1.0/\0'
        if marker==0xe1 and body.startswith(prefix):xmp=body[len(prefix):]
    return exif,xmp

def riff_chunks(data):
    assert data[:4]==b'RIFF' and data[8:12]==b'WEBP' and struct.unpack_from('<I',data,4)[0]+8==len(data)
    at=12;result={}
    while at<len(data):
        kind,n=struct.unpack_from('<4sI',data,at);result[kind]=data[at+8:at+8+n];at+=8+n+n%2
    assert at==len(data);return result

pixels=0;compressed=0
for case in cases:
    path=OUT/case['file'];data=path.read_bytes();im=Image.open(path)
    if path.suffix=='.png':
        chunks=png_chunks(data);exif=chunks[b'eXIf']
        key,rest=chunks[b'iTXt'].split(b'\0',1);assert key==b'XML:com.adobe.xmp'
        flag,method=rest[:2];assert method==0
        language,translated,xmp=rest[2:].split(b'\0',2)
        if flag:xmp=zlib.decompress(xmp);compressed+=1
    elif path.suffix=='.jpg':exif,xmp=jpeg_metadata(data)
    elif path.suffix=='.webp':
        chunks=riff_chunks(data);exif,xmp=chunks[b'EXIF'],chunks[b'XMP ']
        assert chunks[b'VP8X'][0]&0x0c==0x0c
    else:exif=None;xmp=bytes(im.tag_v2[700]);assert len(xmp)>12000
    with zipfile.ZipFile(OUT/case['project']) as z:
        assert z.testzip() is None;manifest=json.loads(z.read('manifest.json'))
        assert manifest['version']==13 and manifest['source_metadata']['xmp'] is True
        assert z.read('metadata/source.xmp')==xmp
        if exif is not None:
            assert manifest['source_metadata']['exif'] is True and z.read('metadata/source.exif')==exif
            assert exif[:2]==(b'II' if case['little'] else b'MM')
            assert im.getexif()[274]==case['orientation'] and im.getexif()[271]=='CameraQA'
            assert z.read('metadata/source.icc')==(ROOT/'data/color/srgb.icc').read_bytes()
            expected=np.asarray(ImageOps.exif_transpose(im).convert('RGBA'))
            actual=np.fromfile(OUT/case['raw'],dtype='u1').reshape(case['height'],case['width'],4)
            np.testing.assert_array_equal(actual,expected,err_msg=case['file'])
            assert (manifest['width'],manifest['height'])==(expected.shape[1],expected.shape[0])
            assert '촬영 테스트'.encode() in xmp
            pixels+=case['width']*case['height']
        else:assert 'metadata/source.exif' not in z.namelist()
with zipfile.ZipFile(OUT/'errors/without-metadata.rphoto') as z:
    manifest=json.loads(z.read('manifest.json'));assert manifest['version']==8 and 'source_metadata' not in manifest
print(f'Source EXIF/XMP: 49 files, 48 PNG/JPEG/WebP oriented images/{pixels} pixels (8 directions and both byte orders), {compressed} compressed PNG XMP packets; independent container/CRC/Make/Unicode metadata and exact Pillow orientation RGBA, project v13 EXIF/XMP/ICC bytes/CRC/dimensions; tiny TIFF packet over 12KB preserved, no-metadata project stays v8.')
