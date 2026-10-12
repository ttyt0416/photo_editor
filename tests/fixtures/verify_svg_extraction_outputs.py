"""Exact encoded file comparison and separate pixel/animation/gzip checks for extracted SVG assets."""
from pathlib import Path
import json, hashlib, gzip, xml.etree.ElementTree as ET, base64, zipfile
from PIL import Image
import numpy as np
ROOT=Path(__file__).resolve().parents[2]/'qa-output/svg-assets/extract'
cases=json.loads((ROOT/'manifest.json').read_text(encoding='utf-8'));results=[]
for i,case in enumerate(cases):
    original=Path(case['input']).read_bytes();saved=Path(case['original']).read_bytes();assert saved==original
    assert Path(case['original']).name==case['name']
    display=Path(case['display']);pixels=np.asarray(Image.open(display).convert('RGBA'))
    assert display.suffix=='.png' or case['name']=='plain.webp'
    if case['name']=='hdr.exr':assert np.array_equal(pixels,np.tile([255,0,102,153],(6,8,1)))
    if case['name']=='exif6.jpg':assert list(pixels.shape[:2])==[12,8]
    if case['name']=='default-image.apng':
        image=Image.open(case['original']);assert image.info.get('default_image') and image.n_frames==3
        assert np.array_equal(pixels,np.tile([191,29,71,255],(6,8,1)))
    if case['name']=='animated.webp':
        image=Image.open(case['original']);assert image.n_frames==3
        assert np.array_equal(pixels,np.tile([191,29,71,255],(6,8,1)))
    with zipfile.ZipFile(ROOT/f'sample-{i}.rphoto') as archive:
        sources=[archive.read(n) for n in archive.namelist() if n.endswith('.svg')];assert len(sources)==1
        node=next(n for n in ET.fromstring(sources[0]).iter() if n.tag.rsplit('}',1)[-1]=='image')
        value=node.attrib.get('data-rphoto-image-original',node.attrib['href']);assert base64.b64decode(value.split(',',1)[1],validate=True)==original
    xml=ET.fromstring(gzip.decompress((ROOT/f'sample-{i}.svgz').read_bytes()));node=next(n for n in xml.iter() if n.tag.rsplit('}',1)[-1]=='image')
    assert base64.b64decode(node.attrib.get('data-rphoto-image-original',node.attrib['href']).split(',',1)[1],validate=True)==original
    results.append({'input':case['name'],'sha256':hashlib.sha256(original).hexdigest(),'bytes':len(saved),'depth':case['depth'],'display_shape':list(pixels.shape)})
nested=ROOT.parent/'extract-nested';tree=ET.parse(nested/'portable/nested.svg');nodes=[n for n in tree.iter() if n.tag.rsplit('}',1)[-1]=='image'];assert len(nodes)==1 and nodes[0].attrib['href'].startswith('data:')
assert (nested/'cancelled.bin').read_bytes()==b'previous';assert (nested/'previous.bin').read_bytes()==b'previous'
(ROOT/'independent.json').write_text(json.dumps(results,indent=2),encoding='utf-8')
print(f'SVG extraction: {len(cases)} byte-identical originals; PNG/EXIF/default and animation frames, projects/gzip, nested portable SVG and cancelled previous output passed.')
