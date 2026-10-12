"""Independent ICC extraction from PNG/JPEG/TIFF and unchanged tagged pixels."""
from pathlib import Path
import json, struct, zipfile, os
os.environ["OPENCV_LOG_LEVEL"]="SILENT"
from PIL import Image
import numpy as np, cv2
ROOT=Path(__file__).resolve().parents[2]/'qa-output/icc-input/output'
cases=json.loads((ROOT/'manifest.json').read_text());assert len(cases)==18
profile=(ROOT/'working-srgb.icc').read_bytes();assert profile[16:20]==b'RGB ' and profile[36:40]==b'acsp'
assert struct.unpack_from('>iii',profile,68)==(0xf6d6,0x10000,0xd32d)
assert struct.unpack_from('>I',profile,0)[0]==len(profile)
for i in range(struct.unpack_from('>I',profile,128)[0]):
    offset,size=struct.unpack_from('>II',profile,136+i*12);assert offset%4==0 and offset+size<=len(profile)
def tiff_profile(data):
    endian='<' if data[:2]==b'II' else '>'
    version=struct.unpack_from(endian+'H',data,2)[0]
    big=version==43;assert version in [42,43]
    offset=struct.unpack_from(endian+('Q' if big else 'I'),data,8 if big else 4)[0]
    n=struct.unpack_from(endian+('Q' if big else 'H'),data,offset)[0];offset+=8 if big else 2
    for i in range(n):
        at=offset+i*(20 if big else 12)
        tag,kind,count=struct.unpack_from(endian+('HHQ' if big else 'HHI'),data,at)
        if tag!=34675:continue
        assert kind in [1,7]
        pointer=struct.unpack_from(endian+('Q' if big else 'I'),data,at+(12 if big else 8))[0]
        return data[pointer:pointer+count] if count>(8 if big else 4) else data[at+(12 if big else 8):at+(12 if big else 8)+count]
    return None
for case in cases:
    path=ROOT/case['file'];data=path.read_bytes()
    if path.suffix=='.tiff':embedded=tiff_profile(data)
    else:embedded=Image.open(path).info.get('icc_profile')
    assert embedded==(profile if case['include'] else None),case['file']
    with zipfile.ZipFile(ROOT/(case['file']+'.rphoto')) as archive:
        assert archive.testzip() is None
        m=json.loads(archive.read('manifest.json'));assert (m['width'],m['height'])==(17,9)
    if case['include']:
        other=ROOT/case['file'].replace('-true.','-false.');assert other!=path and other.exists()
        a=cv2.imdecode(np.frombuffer(data,'u1'),cv2.IMREAD_UNCHANGED)
        b=cv2.imdecode(np.frombuffer(other.read_bytes(),'u1'),cv2.IMREAD_UNCHANGED)
        assert a is not None and b is not None
        np.testing.assert_array_equal(a.view('u4') if a.dtype==np.float32 else a,b.view('u4') if b.dtype==np.float32 else b)
print('ICC output: 18 PNG/JPEG/TIFF files, exact optional sRGB bytes/signature and independent TIFF tag/PNG/JPEG extraction, tagged/untagged pixel bits identical including float TIFF HDR, project ZIP/CRC/size.')
