"""Independent TIFF/EXIF IFD traversal, offsets, values, PNG CRC and Pillow checks."""
from pathlib import Path
import hashlib, json, struct, zipfile, zlib
from PIL import Image
import tifffile

BASE = Path(__file__).resolve().parents[2]
FIXTURES = BASE/'tests/fixtures/tiff-exif'
OUTPUT = BASE/'qa-output/tiff-exif'
cases = json.loads((FIXTURES/'manifest.json').read_text())
assert len(cases) == 48
camera = {270,271,272,274,282,283,296,305,306,315,316,33432,34665,34853,*range(40091,40096)}

def header(data):
    endian = '<' if data[:2] == b'II' else '>'
    big = struct.unpack_from(endian+'H',data,2)[0] == 43
    root = struct.unpack_from(endian+('Q' if big else 'I'),data,8 if big else 4)[0]
    return endian,big,root

def directory(data, at, endian, big):
    countsize, entrysize, inline = (8,20,8) if big else (2,12,4)
    count = struct.unpack_from(endian+('Q' if big else 'H'),data,at)[0]
    fields = {}
    for i in range(count):
        entry = at+countsize+i*entrysize
        tag,kind,n = struct.unpack_from(endian+('HHQ' if big else 'HHI'),data,entry)
        size = {1:1,2:1,3:2,4:4,5:8,6:1,7:1,8:2,9:4,10:8,11:4,12:8,16:8}[kind]*n
        offset = struct.unpack_from(endian+('Q' if big else 'I'),data,entry+4+inline)[0] if size>inline else None
        atvalue = offset if offset is not None else entry+4+inline
        assert atvalue+size <= len(data)
        fields[tag] = (kind,n,data[atvalue:atvalue+size],offset)
    return fields

def pointer(field,endian):
    return int.from_bytes(field[2],'little' if endian=='<' else 'big')

def png_exif(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    at = 8; found = []
    while at < len(data):
        size = int.from_bytes(data[at:at+4],'big'); kind = data[at+4:at+8]
        body = data[at+8:at+8+size]
        assert zlib.crc32(kind+body) == int.from_bytes(data[at+8+size:at+12+size],'big')
        if kind == b'eXIf': found.append(body)
        at += 12+size
    assert len(found) <= 1
    return found[0] if found else None

for case in cases:
    name = case['file']; original = (FIXTURES/name).read_bytes()
    assert hashlib.sha256(original).hexdigest() == case['sha256']
    endian,big,root = header(original)
    assert big == case['big'] and endian == case['endian'] and root == case['root']
    original_root = directory(original,root,endian,big)
    with zipfile.ZipFile(OUTPUT/(name+'.rphoto')) as project:
        assert project.testzip() is None
        metadata = json.loads(project.read('manifest.json'))
        width,height = (5,3) if case['orientation']==1 else (3,5)
        assert metadata['version'] == 13 and (metadata['width'],metadata['height']) == (width,height)
        assert metadata['layers'][0]['bit_depth'] == {'rgb8':8,'planar16':16,'gray32':32,'palette':16}[case['mode']]
        saved = project.read('metadata/source.exif')
    e,b,r = header(saved); assert e==endian and not b and r==root
    captured_root = directory(saved,r,e,b)
    assert set(captured_root) == set(original_root) & camera
    for tag,field in captured_root.items():
        if tag in (34665,34853):
            assert field[:2] == (4,1) and pointer(field,e)==pointer(original_root[tag],e)
        else: assert field[:3] == original_root[tag][:3], (name,tag)
    for at in [case['exif'],case['gps'],case['interop']]:
        source = directory(original,at,endian,big)
        captured = directory(saved,at,endian,False)
        assert set(source) == set(captured)
        for tag,field in source.items():
            if tag==40965: assert pointer(field,e)==pointer(captured[tag],e)
            else:
                assert field[:3]==captured[tag][:3],(name,tag)
                # Every original external block, including MakerNote, stays put.
                if field[3] is not None: assert field[3]==captured[tag][3],(name,tag)
    with tifffile.TiffFile(FIXTURES/name) as file:
        for at,length in zip(file.pages[0].dataoffsets,file.pages[0].databytecounts):
            assert saved[at:at+length] == bytes(length), 'image payload must not be copied into EXIF'
    assert captured_root[271][2] == b'CameraQA\0'
    exif = directory(saved,case['exif'],endian,False)
    assert exif[37500][2] == b'MakerQA opaque data\0'
    assert exif[37510][2] == b'UNICODE\0'+b'\xfe\xff'+'촬영 테스트'.encode('utf-16-be')
    assert struct.unpack(endian+'II',exif[33434][2]) == (1,125)
    for include in [True,False]:
        path = OUTPUT/(name+f'-exif{str(include).lower()}.png')
        emitted = png_exif(path.read_bytes())
        if not include: assert emitted is None; continue
        e,b,r = header(emitted)
        assert e==endian and not b and r==root
        output_root = directory(emitted,r,e,b)
        assert pointer(output_root[274],e) == 1
        emitted_exif = directory(emitted,case['exif'],e,b)
        assert pointer(emitted_exif[40962],e)==width and pointer(emitted_exif[40963],e)==height
        for tag in exif:
            if tag not in (40962,40963): assert emitted_exif[tag] == exif[tag]
        assert directory(emitted,case['gps'],e,b) == directory(saved,case['gps'],e,b)
        with Image.open(path) as image:
            parsed = image.getexif()
            assert parsed[271] == 'CameraQA' and parsed[272] == 'ModelQA' and parsed[274] == 1
            nested = parsed.get_ifd(34665)
            assert nested[36867] == '2026:10:12 14:30:00' and nested[42036] == 'LensQA 35mm'
            assert float(nested[33434]) == 1/125 and float(nested[33437]) == 2.8
            gps = parsed.get_ifd(34853)
            assert gps[1]=='N' and gps[3]=='E' and tuple(map(float,gps[2])) == (37.,33.,0.)
print('TIFF EXIF: 48 classic/BigTIFF RGB8/planar16/GRAY32/palette inputs, both endian and 3 orientations; all camera/EXIF/GPS/interop value bytes, original external block offsets including MakerNote, Unicode comments/project ZIP/CRC/depth retained; zero image payload; PNG on/off CRC and orientation/dimensions plus independent Pillow camera/lens/exposure/GPS checks.')
