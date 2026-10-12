"""Independent source geometry/resources and project/native gzip preservation checks."""
from pathlib import Path
import base64,gzip,xml.etree.ElementTree as ET,zipfile,json
ROOT=Path(__file__).resolve().parents[2]/'qa-output/svg-assets/replace'
before=ET.parse(ROOT/'before.svg').getroot();after=ET.parse(ROOT/'after.svg').getroot()
def by_id(root,name):return next(n for n in root.iter() if n.attrib.get('id')==name)
a=by_id(before,'target');b=by_id(after,'target')
for key in ['id','x','y','width','height','transform','mask','style','preserveAspectRatio']:
    assert a.attrib.get(key)==b.attrib.get(key),(key,a.attrib,b.attrib)
assert ET.tostring(by_id(before,'other'))==ET.tostring(by_id(after,'other'))
for tag in ['style','defs']:
    n=lambda root:next(n for n in root.iter() if n.tag.rsplit('}',1)[-1]==tag)
    assert ET.tostring(n(before))==ET.tostring(n(after))
assert b.attrib['href']==b.attrib['{http://www.w3.org/1999/xlink}href']
def blob(value):return base64.b64decode(value.split(',',1)[1],validate=True)
original=(ROOT/'다음 사진.bmp').read_bytes();assert blob(b.attrib['data-rphoto-image-original'])==original
assert b.attrib['data-rphoto-image-name']=='다음 사진.bmp'
assert b'<!-- exact -->' in (ROOT/'after.svg').read_bytes()
with zipfile.ZipFile(ROOT/'replaced.rphoto') as project:
    sources=[project.read(n) for n in project.namelist() if n.endswith('.svg')];assert len(sources)==1
    assert sources[0]==(ROOT/'after.svg').read_bytes()
output=ET.fromstring(gzip.decompress((ROOT/'replaced.svgz').read_bytes()))
resources=[n for n in output.iter() if 'data-rphoto-image-original' in n.attrib];assert len(resources)==1
assert blob(resources[0].attrib['data-rphoto-image-original'])==original
(ROOT/'independent.json').write_text(json.dumps({'geometry':9,'other_image':True,'styles_defs':True,'original_bytes':len(original),'project_exact_source':True,'svgz_original':True},indent=2),encoding='utf-8')
print('SVG replacement: 9 geometry/style attributes, untouched other image/defs/CSS/comments, exact original/project source/native gzip passed.')
