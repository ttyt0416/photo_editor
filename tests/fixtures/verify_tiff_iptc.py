"""Independent native TIFF tag bytes, IPTC datasets and Photoshop resource parsing."""
from pathlib import Path
import hashlib,json,struct,zipfile
import tifffile

BASE=Path(__file__).resolve().parents[2]
FIXTURES=BASE/'tests/fixtures/tiff-iptc'
OUTPUT=BASE/'qa-output/tiff-iptc'
cases=json.loads((FIXTURES/'manifest.json').read_text());assert len(cases)==80
reference=(FIXTURES/'reference.iim').read_bytes()
resources=(FIXTURES/'reference.irb').read_bytes()

def datasets(data):
    at=0;values=[]
    while at<len(data):
        assert data[at]==0x1c
        record,tag=data[at+1:at+3];size=int.from_bytes(data[at+3:at+5],'big');at+=5
        assert at+size<=len(data)
        values.append((record,tag,data[at:at+size]));at+=size
    return values

fields=datasets(reference)
assert fields[0]==(1,90,b'\x1b%G')
assert [v.decode() for r,t,v in fields if (r,t)==(2,25)]==['keyword','촬영']
assert [v.decode() for r,t,v in fields if (r,t)==(2,120)]==['한글 설명\n두 번째 줄']

def irb(data):
    at=0;values=[]
    while at<len(data):
        assert data[at:at+4]==b'8BIM';identifier=int.from_bytes(data[at+4:at+6],'big');at+=6
        count=data[at];at+=(count+2)//2*2
        size=int.from_bytes(data[at:at+4],'big');at+=4
        assert at+size<=len(data);values.append((identifier,data[at:at+size]));at+=(size+1)//2*2
    return values

assert irb(resources)==[(0x03ee,b'unknown\0\xff'),(0x0404,reference),(0x0424,b'<xmp>preserve all resources</xmp>')]
for case in cases:
    name=case['file'];data=(FIXTURES/name).read_bytes()
    assert hashlib.sha256(data).hexdigest()==case['sha256']
    expected_iptc=None if case['raw'] is None else bytes.fromhex(case['raw'])
    with tifffile.TiffFile(FIXTURES/name) as file:
        page=file.pages[0];assert file.is_bigtiff==case['big']
        endian=case['endian'];cs,es,inline=(8,20,8) if case['big'] else (2,12,4)
        count=struct.unpack_from(endian+('Q' if case['big'] else 'H'),data,page.offset)[0]
        raw_tags={}
        for i in range(count):
            at=page.offset+cs+i*es
            tag,kind,n=struct.unpack_from(endian+('HHQ' if case['big'] else 'HHI'),data,at)
            if tag not in (33723,34377):continue
            size=n*(4 if kind==4 else 1)
            pointer=struct.unpack_from(endian+('Q' if case['big'] else 'I'),data,at+4+inline)[0] if size>inline else at+4+inline
            raw_tags[tag]=data[pointer:pointer+size]
            assert kind==(case['kind'] if tag==33723 else 1)
        assert raw_tags.get(33723)==expected_iptc
        assert raw_tags.get(34377)==(resources if case['photoshop'] else None)
    with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        metadata=json.loads(project.read('manifest.json'));assert metadata['version']==14
        assert metadata['layers'][0]['bit_depth']=={'rgb8':8,'planar16':16,'gray32':32,'palette':16}[case['mode']]
        record=metadata['source_metadata']
        assert record.get('iptc',False)==(expected_iptc is not None)
        assert record.get('photoshop',False)==case['photoshop']
        if expected_iptc is not None:
            stored=project.read('metadata/source.iptc');assert stored==expected_iptc
            assert record['iptc_tiff']=={'kind':case['kind'],'little':case['endian']=='<'}
            if case['kind']==4:
                words=struct.unpack(case['endian']+'I'*(len(stored)//4),stored)
                restored=struct.pack('>'+'I'*len(words),*words)
                assert restored[:len(reference)]==reference and not any(restored[len(reference):])
            else:assert stored==reference
        else:assert 'metadata/source.iptc' not in project.namelist() and 'iptc_tiff' not in record
        if case['photoshop']:
            preserved=project.read('metadata/source.photoshop');assert preserved==resources
            assert irb(preserved)[1][1]==reference
        else:assert 'metadata/source.photoshop' not in project.namelist()
print('TIFF IPTC: 80 classic/BigTIFF RGB8/planar16/GRAY32/palette inputs, both endian, BYTE/UNDEFINED/LONG/IPTC-only/Photoshop-only/both; native tag payload bytes +type/word order exact in project v14/CRC/depth; independent IIM UTF-8/repeated keyword/caption/binary and Photoshop name/data padding/unknown/XMP/IPTC resources retained.')
