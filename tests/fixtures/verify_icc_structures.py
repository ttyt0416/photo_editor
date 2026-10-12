"""Independent ICC header/tag-layout checks; color interpretation is checked separately.
ICC requirements: https://www.color.org/profiles/assessment/
"""
from pathlib import Path
import json,struct
from PIL import ImageCms,Image
import io
import numpy as np

BASE=Path(__file__).resolve().parents[2]
ROOT=BASE/'qa-output/icc-structure'
names=json.loads((ROOT/'manifest.json').read_text());assert len(names)==6

def check(data):
    assert len(data)>=132 and len(data)%4==0
    assert struct.unpack_from('>I',data)[0]==len(data)
    assert data[36:40]==b'acsp' and data[20:24]==b'XYZ '
    assert struct.unpack_from('>iii',data,68)==(0xf6d6,0x10000,0xd32d)
    count=struct.unpack_from('>I',data,128)[0]
    end=132+12*count;result={};ranges=[]
    for i in range(count):
        tag,at,size=struct.unpack_from('>4sII',data,132+12*i)
        assert at%4==0 and at>=end and at+size<=len(data) and size>=8
        assert tag not in result
        assert data[at+4:at+8]==bytes(4)
        result[tag]=data[at:at+size];ranges.append((at,at+size))
    # Shared TRCs are allowed; partial overlapping elements are not.
    for a,b in ranges:
        for c,d in ranges:
            assert (a,b)==(c,d) or b<=c or d<=a
    assert len(data)==(max(b for a,b in ranges)+3)//4*4
    return result

working=(BASE/'data/color/srgb.icc').read_bytes()
assert len(check(working))==11
assert len(check((BASE/'data/color/srgb-gray.icc').read_bytes()))==4
for name in names:
    raw=(ROOT/(name+'-raw.icc')).read_bytes()
    packed=(ROOT/(name+'.icc')).read_bytes()
    repaired=check(packed)
    count=struct.unpack_from('>I',raw,128)[0]
    for i in range(count):
        tag,at,size=struct.unpack_from('>4sII',raw,132+12*i)
        assert repaired[tag]==raw[at:at+size]
    # The upstream serializer's D50 omission is an explicit negative case.
    assert struct.unpack_from('>iii',raw,68)!=(0xf6d6,0x10000,0xd32d)
    input=Image.fromarray(np.array([[0,1,64,128,255]],dtype='u1')) if name.startswith('gray') else Image.fromarray(np.array([[[0,0,0],[1,1,1],[64,128,192],[255,255,255]]],dtype='u1'))
    transform=ImageCms.buildTransformFromOpenProfiles(ImageCms.ImageCmsProfile(io.BytesIO(packed)),ImageCms.ImageCmsProfile(io.BytesIO(working)),input.mode,'RGB',renderingIntent=1,flags=ImageCms.Flags.NOOPTIMIZE)
    assert ImageCms.applyTransform(input,transform).size==input.size
profiles=0
for folder in ['icc-input','icc-integer','icc-float','icc-float-sdr','jpeg2000-input/icc']:
    for path in (BASE/'qa-output'/folder).glob('*.icc'):
        check(path.read_bytes());profiles+=1
assert profiles==41,profiles
print(f'ICC structure: 2 unchanged working profiles +6 repacked/raw negative pairs +{profiles} generated import profiles; exact size/D50/tag alignment/reserved bytes/bounds/no partial overlap/final padding, every original tag payload unchanged; LittleCMS opens 6 repaired RGB/GRAY profiles. This is a structural subset, not full ICC conformance.')
