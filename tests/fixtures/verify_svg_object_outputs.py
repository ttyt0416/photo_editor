"""Independently check native SVG geometry/reference preservation after canvas editing."""
from pathlib import Path
from zipfile import ZipFile
import json
import re
import xml.etree.ElementTree as ET
root=Path(__file__).resolve().parents[2]/'qa-output/svg-objects'
ns={'s':'http://www.w3.org/2000/svg'}
output=ET.fromstring((root/'native.svg').read_text(encoding='utf-8'))
for tag,count in [('rect',1),('circle',2),('ellipse',1),('line',1),('polyline',1),('polygon',1),('symbol',1),('use',3)]:
    assert len(output.findall('.//s:'+tag,ns))==count,(tag,count)
ids={n.attrib['id']:n for n in output.iter() if 'id' in n.attrib}
assert len(ids)==sum('id' in n.attrib for n in output.iter())
for node in output.findall('.//s:use',ns):
    assert node.attrib['href'][1:] in ids
    assert node.attrib['href'].startswith('#')
with ZipFile(root/'native8.rphoto') as archive:
    manifest=json.loads(archive.read('manifest.json'))
    layer=next(l for l in manifest['layers'] if l.get('svg_source'))
    source=archive.read(f"layers/{layer['id']}-source.svg").decode('utf-8')
xml=ET.fromstring(source)
by_id={n.attrib['id']:n for n in xml.iter() if 'id' in n.attrib}
assert by_id['rect'].attrib['x']=='20' and by_id['rect'].attrib['rx']=='3'
assert by_id['circle'].attrib['r']=='10'
assert by_id['ellipse'].attrib['rx']=='12' and by_id['ellipse'].attrib['ry']=='8'
assert by_id['template'].attrib['d']=='M0 0L12 0L12 12L0 12Z'
assert by_id['use'].attrib['href']=='#template' and by_id['use2'].attrib['href']=='#template'
assert by_id['use'].attrib['transform'].startswith('matrix(')
assert 'transform' not in by_id['use2'].attrib
assert len([n for n in xml.iter() if n.attrib.get('data-photo-editor-expanded-viewport')=='1'])==1
assert '<!--keep-->' in source and '.paint{fill:#2468ac;stroke:#fff;stroke-width:2}' in source
print('native SVG canvas outputs: original geometry, instances/templates, IDs/references, single viewport and source styles/comments verified')
