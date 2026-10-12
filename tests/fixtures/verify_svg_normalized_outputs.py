"""Separate PNG/EXIF/animation goldens and XML/gzip/exact-source checks; no GUI."""
from pathlib import Path
import base64, gzip, io, json, xml.etree.ElementTree as ET, zipfile
import numpy as np
from PIL import Image
ROOT=Path(__file__).resolve().parents[2]/'qa-output/svg-assets'
root=ROOT/'normalized';cases=json.loads((root/'manifest.json').read_text(encoding='utf-8'));results=[]
def payload(value):
    header,data=value.split(',',1);assert header.endswith(';base64');return base64.b64decode(data,validate=True)
for index,case in enumerate(cases):
    original=Path(case['input']).read_bytes();xml=gzip.decompress((root/case['file']).read_bytes());tree=ET.fromstring(xml)
    nodes=[e for e in tree.iter() if e.tag.rsplit('}',1)[-1]=='image'];assert len(nodes)==1
    node=nodes[0];assert payload(node.attrib.get('data-rphoto-image-original',node.attrib['href']))==original
    expected=np.array(Image.open(case['expected']).convert('RGBA'),dtype=np.int16);actual=np.array(Image.open(root/case['actual']).convert('RGBA'),dtype=np.int16)
    assert actual.shape==expected.shape;error=int(np.max(np.abs(actual-expected)));assert error<=2,(case,error)
    png=np.array(Image.open(io.BytesIO(payload(node.attrib['href']))).convert('RGBA'),dtype=np.int16);assert np.max(np.abs(png-actual))<=1
    with zipfile.ZipFile(root/f'sample-{index}.rphoto') as project:
        source=[project.read(n) for n in project.namelist() if n.endswith('.svg')];assert len(source)==1
        n=[e for e in ET.fromstring(source[0]).iter() if e.tag.rsplit('}',1)[-1]=='image'][0]
        assert payload(n.attrib.get('data-rphoto-image-original',n.attrib['href']))==original
    results.append({'input':str(case['input']),'error':error,'bytes':len(original),'shape':actual.shape})
compressed=ROOT/'compressed';plain=gzip.decompress((compressed/'output.SVGZ').read_bytes());ET.fromstring(plain);assert b'<!-- retained -->' in plain
assert b'url(' not in plain or b'#' in plain
for name in ['bad-0.svgz','bad-1.svgz','bad-2.svgz']:
    try:gzip.decompress((compressed/name).read_bytes())
    except (OSError,EOFError):pass
    else:raise AssertionError(name)
(root/'independent.json').write_text(json.dumps(results,indent=2),encoding='utf-8')
print(f'SVG normalized images: {len(cases)} cases; EXIF/goldens/first or default canvas, exact originals/project/gzip/XML passed.')
