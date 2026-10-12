"""Independent APP1 offsets/MD5/link, original XML and project preservation."""
from pathlib import Path
import hashlib,io,json,zipfile,xml.etree.ElementTree as ET
import numpy as np
from PIL import Image,ImageOps
BASE=Path(__file__).resolve().parents[2]
FIXTURES=BASE/'tests/fixtures/jpeg-extended-xmp';OUTPUT=BASE/'qa-output/jpeg-extended-xmp'
cases=json.loads((FIXTURES/'manifest.json').read_text());assert len(cases)==20
NOTE='{http://ns.adobe.com/xmp/note/}HasExtendedXMP'
SIG=b'http://ns.adobe.com/xmp/extension/\0'
def app1(data):
    assert data[:2]==b'\xff\xd8';at=2;values=[]
    while at<len(data):
        assert data[at]==255
        while data[at]==255:at+=1
        marker=data[at];at+=1
        if marker in [0xda,0xd9]:break
        if marker==1 or 0xd0<=marker<=0xd7:continue
        size=int.from_bytes(data[at:at+2],'big');assert size>=2;payload=data[at+2:at+size];at+=size
        if marker==0xe1:values.append(payload)
    return values
for case in cases:
    name=case['file'];data=(FIXTURES/name).read_bytes();assert hashlib.sha256(data).hexdigest()==case['sha256']
    packet=(FIXTURES/case['packet']).read_bytes();basic=(FIXTURES/(name+'.basic.xmp')).read_bytes()
    assert hashlib.md5(packet).hexdigest().upper()==case['guid']
    xml=ET.fromstring(basic);links=[node.attrib[NOTE] for node in xml.iter() if NOTE in node.attrib]+[node.text for node in xml.iter(NOTE)]
    assert links==[case['guid']]
    ET.fromstring(packet)
    chunks=[]
    for payload in app1(data):
        if not payload.startswith(SIG):continue
        p=payload[len(SIG):];guid=p[:32].decode();total=int.from_bytes(p[32:36],'big');offset=int.from_bytes(p[36:40],'big')
        assert guid==case['guid'] and total==len(packet) and offset+len(p[40:])<=total
        chunks.append((offset,p[40:]))
    assert len(chunks)==case['chunks']
    restored=bytearray();
    for offset,part in sorted(chunks):assert offset==len(restored);restored.extend(part)
    assert bytes(restored)==packet
    if case['layout'] in ['reverse','basic-late']:assert len(chunks)>1 and chunks[0][0]>chunks[-1][0]
    if case['layout']=='split-utf8':
        try:chunks[0][1].decode();assert False,'first chunk should split a UTF-8 code point'
        except UnicodeDecodeError:pass
    with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
        assert project.testzip() is None;m=json.loads(project.read('manifest.json'))
        assert m['version']==15 and m['source_metadata']['xmp_extended'] is True and m['source_metadata']['xmp_extended_guid']==case['guid']
        assert (m['width'],m['height'])==(3,5) and m['layers'][0]['bit_depth']==8
        assert project.read('metadata/source.xmp')==basic and project.read('metadata/source-extended.xmp')==packet
        assert project.read('metadata/source.photoshop')==(BASE/'tests/fixtures/jpeg-iptc/small.irb').read_bytes()
        layer=m['layers'][0]['id']
        with Image.open(io.BytesIO(project.read(f'layers/{layer}.png'))) as image:actual=np.asarray(image.convert('RGBA'))
    with Image.open(BASE/'tests/fixtures/jpeg-iptc'/case['base']) as image:expected=np.asarray(ImageOps.exif_transpose(image).convert('RGBA'))
    assert np.abs(actual.astype('i4')-expected.astype('i4')).max()<=2
    emitted=app1((OUTPUT/(name+'-no-xmp.jpg')).read_bytes())
    assert not any(p.startswith(SIG) or p.startswith(b'http://ns.adobe.com/xap/1.0/\0') for p in emitted)
print('ExtendedXMP: 20 RGB/GRAY baseline/progressive JPEGs, single/UTF-8 split/reverse/225282-byte/before-basic chunks; independent APP1 coverage/MD5/namespaced basic GUID/XML, exact basic+extended+Photoshop project v15/CRC/depth, Pillow pixels within 2; XMP-disabled output has no dangling extension link.')
