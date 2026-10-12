"""Original encoded ICC bytes vs native project ZIP assets, independently."""
from pathlib import Path
from PIL import Image
import json,struct,zipfile,hashlib
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'qa-output/source-icc'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==8

def boxes(data):
    at=0;result={}
    while at<len(data):
        n,kind=struct.unpack_from('>I4s',data,at)
        assert n>=8 and at+n<=len(data) and kind not in result
        result[kind]=data[at+8:at+n];at+=n
    assert at==len(data);return result

for case in cases:
    path=OUT/case['source']
    if path.suffix=='.jp2':
        header=boxes(boxes(path.read_bytes())[b'jp2h']);assert header[b'colr'][:3]==bytes([2,0,0]);profile=header[b'colr'][3:]
    elif path.suffix=='.tiff':profile=bytes(Image.open(path).tag_v2[34675])
    else:profile=Image.open(path).info['icc_profile']
    if case['profile'] in ['rgb','gray']:
        assert profile==(ROOT/'data/color'/('srgb-gray.icc' if case['profile']=='gray' else 'srgb.icc')).read_bytes()
    if case['profile']=='invalid':assert profile==bytes([19])*128
    with zipfile.ZipFile(OUT/case['project']) as z:
        assert z.testzip() is None
        manifest=json.loads(z.read('manifest.json'));assert manifest['version']==12 and manifest['source_icc'] is True
        assert z.read('metadata/source.icc')==profile
        assert z.getinfo('metadata/source.icc').file_size==len(profile)<=16*1024*1024
    if case['profile'] in ['p3','invalid']:
        exported=Image.open(OUT/(case['profile']+'-working.png')).info['icc_profile']
        assert exported==(ROOT/'data/color/srgb.icc').read_bytes() and exported!=profile
with zipfile.ZipFile(OUT/'errors/without-profile.rphoto') as z:
    manifest=json.loads(z.read('manifest.json'));assert manifest['version']==8 and 'source_icc' not in manifest
    assert 'metadata/source.icc' not in z.namelist()
assert (OUT/'errors/valid.rphoto').is_file()
print('Source ICC preservation: 8 PNG/JPEG/TIFF/WebP/JP2 RGB/GRAY/P3/invalid imports; original encoded profile bytes exactly equal project v12 asset, source sizes/CRC/manifest checked independently; working PNG retains working sRGB rather than source label, no-profile project remains v8.')
