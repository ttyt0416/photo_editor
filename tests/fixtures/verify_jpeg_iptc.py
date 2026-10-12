"""Independent JPEG APP13 stream, Photoshop resources and project pixels."""
from pathlib import Path
import hashlib,io,json,zipfile
import numpy as np
from PIL import Image,ImageOps
BASE=Path(__file__).resolve().parents[2]
FIXTURES=BASE/'tests/fixtures/jpeg-iptc'
OUTPUT=BASE/'qa-output/jpeg-iptc'
cases=json.loads((FIXTURES/'manifest.json').read_text());assert len(cases)==24

def app13(data):
    assert data[:2]==b'\xff\xd8';at=2;blocks=[]
    while at<len(data):
        assert data[at]==255
        while data[at]==255:at+=1
        marker=data[at];at+=1
        if marker in [0xda,0xd9]:break
        if marker==1 or 0xd0<=marker<=0xd7:continue
        length=int.from_bytes(data[at:at+2],'big');assert length>=2
        payload=data[at+2:at+length];assert len(payload)==length-2;at+=length
        if marker==0xed and payload.startswith(b'Photoshop 3.0\0'):blocks.append(payload[14:])
    return blocks

def resources(data):
    at=0;result=[]
    while at<len(data):
        assert data[at:at+4]==b'8BIM';identifier=int.from_bytes(data[at+4:at+6],'big');at+=6
        name=data[at];at+=(name+2)//2*2
        length=int.from_bytes(data[at:at+4],'big');at+=4
        assert at+length<=len(data);result.append((identifier,data[at:at+length]));at+=(length+1)//2*2
    return result

for case in cases:
    name=case['file'];original=(FIXTURES/name).read_bytes()
    assert hashlib.sha256(original).hexdigest()==case['sha256']
    blocks=app13(original);assert len(blocks)==case['segments']
    expected=(FIXTURES/case['resource']).read_bytes();assert b''.join(blocks)==expected
    parsed=resources(expected)
    if parsed:
        assert [identifier for identifier,value in parsed[:3]]==[0x03ee,0x0404,0x0424]
        assert parsed[1][1]==(FIXTURES.parent/'tiff-iptc/reference.iim').read_bytes()
        if case['layout']=='large':assert len(parsed[-1][1])==100005
    with Image.open(FIXTURES/name) as image:
        assert image.getexif()[271]=='CameraQA' and image.getexif()[274]==6
        pixel_reference=np.asarray(ImageOps.exif_transpose(image).convert('RGBA'))
    with Image.open(FIXTURES/case['base']) as image:
        np.testing.assert_array_equal(pixel_reference,np.asarray(ImageOps.exif_transpose(image).convert('RGBA')))
    with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        metadata=json.loads(project.read('manifest.json'))
        assert metadata['version']==14 and metadata['source_metadata']['photoshop'] is True
        assert (metadata['width'],metadata['height'])==(3,5) and metadata['layers'][0]['bit_depth']==8
        assert project.read('metadata/source.photoshop')==expected
        assert not metadata['source_metadata'].get('iptc',False) and 'iptc_tiff' not in metadata['source_metadata']
        assert project.read('metadata/source.exif').startswith((b'II',b'MM'))
        layer=metadata['layers'][0]['id']
        with Image.open(io.BytesIO(project.read(f'layers/{layer}.png'))) as image:
            actual=np.asarray(image.convert('RGBA'))
        difference=np.abs(actual.astype('i4')-pixel_reference.astype('i4')).max()
        assert difference<=2,(name,difference)
print('JPEG APP13: 24 RGB/GRAY baseline/progressive JPEGs, single/resource-boundary/arbitrary splits/100005-byte resource/other APP13+fill/empty; exact ordered Photoshop/IPTC/unknown/XMP resource bytes/project v14/CRC/EXIF orientation and native8 depth; identical source metadata-on/off Pillow pixels and app pixels within 2 decoder units.')
