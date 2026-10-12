"""Independent JPEG ExtendedXMP output MD5/chunks/geometry and coding bytes."""
from pathlib import Path
import hashlib,json,zipfile,xml.etree.ElementTree as ET
import numpy as np
from PIL import Image
BASE=Path(__file__).resolve().parents[2]
ROOT=BASE/'qa-output/jpeg-extended-output'
SIG=b'http://ns.adobe.com/xmp/extension/\0';BASIC=b'http://ns.adobe.com/xap/1.0/\0'
NOTE='{http://ns.adobe.com/xmp/note/}HasExtendedXMP'
def packets(data):
    at=2;basic=None;parts=[];stripped=data[:2]
    while at<len(data):
        start=at;assert data[at]==255
        while data[at]==255:at+=1
        marker=data[at];at+=1
        if marker in (0xda,0xd9):stripped+=data[start:];break
        size=int.from_bytes(data[at:at+2],'big');assert size>=2;payload=data[at+2:at+size];at+=size
        if marker!=0xe1:stripped+=data[start:at]
        elif payload.startswith(BASIC):assert basic is None;basic=payload[len(BASIC):]
        elif payload.startswith(SIG):
            p=payload[len(SIG):];assert len(payload)<=65533
            parts.append((p[:32].decode(),int.from_bytes(p[32:36],'big'),int.from_bytes(p[36:40],'big'),p[40:]))
    if basic is None:assert not parts;return None,None,None,stripped
    xml=ET.fromstring(basic)
    links=[n.attrib[NOTE] for n in xml.iter() if NOTE in n.attrib]+[n.text for n in xml.iter(NOTE)]
    assert len(links)==1;guid=links[0];assert len(parts)>0
    result=bytearray();total=parts[0][1]
    for identifier,size,offset,value in parts:
        assert identifier==guid and size==total and offset==len(result) and len(value)<=65458
        result.extend(value)
    assert len(result)==total and hashlib.md5(result).hexdigest().upper()==guid
    ET.fromstring(result)
    return basic,bytes(result),guid,stripped
def geometry(data,width,height):
    xml=ET.fromstring(data)
    expected={'{http://ns.adobe.com/tiff/1.0/}Orientation':'1','{http://ns.adobe.com/tiff/1.0/}ImageWidth':str(width),'{http://ns.adobe.com/tiff/1.0/}ImageLength':str(height),'{http://ns.adobe.com/exif/1.0/}PixelXDimension':str(width),'{http://ns.adobe.com/exif/1.0/}PixelYDimension':str(height)}
    for node in xml.iter():
        for tag,value in node.attrib.items():
            if tag in expected:assert value==expected[tag]
        if node.tag in expected:assert ''.join(node.itertext()).strip()==expected[node.tag]
cases=json.loads((ROOT/'manifest.json').read_text());assert len(cases)==80
for case in cases:
    path=ROOT/case['file'];basic,extended,guid,coding=packets(path.read_bytes())
    if not case['include']:assert basic is None;continue
    geometry(basic,3,5);geometry(extended,3,5)
    source=BASE/'tests/fixtures/jpeg-extended-xmp'
    reference=json.loads((source/'manifest.json').read_text())
    ref=next(v for v in reference if v['file']==case['source'])
    assert extended==(source/ref['packet']).read_bytes()
    other=ROOT/case['file'].replace('-xtrue.','-xfalse.');assert other.exists()
    assert coding==packets(other.read_bytes())[3]
    with Image.open(path) as a,Image.open(other) as b:
        np.testing.assert_array_equal(np.asarray(a.convert('RGBA')),np.asarray(b.convert('RGBA')))
        assert a.size==(3,5)
    with zipfile.ZipFile(ROOT/(case['file']+'.rphoto')) as project:
        assert project.testzip() is None;m=json.loads(project.read('manifest.json'))
        assert m['version']==15 and m['source_metadata']['xmp_extended_guid']==guid
        assert project.read('metadata/source.xmp')==basic and project.read('metadata/source-extended.xmp')==extended
G=BASE/'qa-output/jpeg-extended-geometry'
for element in [False,True]:
    name='element'+str(element).lower()
    source=(G/(name+'-source-extended.xmp')).read_bytes()
    basic,extended,guid,_=packets((G/(name+'.jpg')).read_bytes())
    geometry(basic,9,11);geometry(extended,9,11)
    assert guid!=hashlib.md5(source).hexdigest().upper()
    assert b'<!--keep-->' in extended and b"<unknown attr='preserve'>opaque</unknown>" in extended and b'<!--basic-->' in basic
print('ExtendedXMP output: 80 JPEGs/40 metadata-on/off pairs, baseline+progressive, independent APP1 65458-byte chunk bounds/ordered coverage/MD5/GUID/XML/project v15 CRC; coding bytes and Pillow pixels identical; 2 attribute/CDATA links with both-packet geometry9x11/orientation1/new GUID and unchanged unknown/comments/source checked.')
