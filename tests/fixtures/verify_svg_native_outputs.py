"""Independent XML/ID/CSS scope and original project source checks for native SVG output."""
from pathlib import Path
from zipfile import ZipFile
import json
import re
import xml.etree.ElementTree as ET
root = Path(__file__).resolve().parents[2] / 'qa-output/svg-native'
ns = {'s': 'http://www.w3.org/2000/svg'}
svg = ET.fromstring((root / 'native.svg').read_text(encoding='utf-8'))
assert (svg.attrib['width'], svg.attrib['height']) == ('80', '40')
for name, count in [('rect', 4), ('circle', 2), ('use', 2), ('metadata', 2), ('style', 2)]:
    assert len(svg.findall('.//s:' + name, ns)) == count
ids = [node.attrib['id'] for node in svg.iter() if 'id' in node.attrib]
assert len(ids) == len(set(ids))
for node in svg.iter():
    href = node.attrib.get('href', '')
    if href.startswith('#'):
        assert href[1:] in ids
    for value in node.attrib.values():
        for ref in re.findall(r'url\([\s\"\']*#([^\s\"\')]+)', value):
            assert ref in ids
for style in svg.findall('.//s:style', ns):
    css = style.text
    assert '#photo-editor-layer-' in css
    assert '[data-photo-editor-root-layer-' in css
    assert 'stroke:#fff' in css
    for ref in re.findall(r'url\([\s\"\']*#([^\s\"\')]+)', css):
        assert ref in ids
assert '<!-- original comment -->' in (root / 'native.svg').read_text(encoding='utf-8')
with ZipFile(root / 'native.rphoto') as project:
    manifest = json.loads(project.read('manifest.json'))
    for layer in manifest['layers']:
        original = project.read(f"layers/{layer['id']}-source.svg").decode('utf-8')
        assert 'photo-editor-layer-' not in original
        assert 'id="shape"' in original and 'href="#template"' in original
        assert '<circle' in original and '<!-- original comment -->' in original
print('native SVG: original shapes/styles/metadata/comments, unique IDs, references, CSS scopes and unchanged project assets verified')
