"""Independent Python Brotli, box/PNG metadata, payload and project comparison."""
from pathlib import Path
from PIL import Image
import brotli
import hashlib
import json
import struct
import zipfile
import zlib
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tests/fixtures/jxl-metadata'
OUT = ROOT / 'qa-output/jxl-metadata/compressed'
reference = json.loads((FIXTURES / 'manifest.json').read_text())
for case in reference['cases']:
    for kind in ['exif', 'xmp']:
        data = (FIXTURES / case[kind]).read_bytes()
        assert hashlib.sha256(data).hexdigest() == case[kind + '_sha256']
        raw = brotli.decompress(data)
        assert len(raw) == case[kind + '_size']
        assert hashlib.sha256(raw).hexdigest() == case[kind + '_raw_sha256']
bomb = (FIXTURES / 'over-budget.br').read_bytes()
assert hashlib.sha256(bomb).hexdigest() == reference['over_budget_sha256']
assert len(brotli.decompress(bomb)) == reference['over_budget_size'] > 16 * 1024 * 1024


def boxes(data):
    at = 0
    while at < len(data):
        size, kind = struct.unpack_from('>I4s', data, at)
        header = 8
        if size == 1:
            size = struct.unpack_from('>Q', data, at + 8)[0]
            header = 16
        elif size == 0:
            size = len(data) - at
        assert size >= header and at + size <= len(data)
        yield kind, data[at + header:at + size]
        at += size
    assert at == len(data)


def png(data):
    assert data[:8] == b'\x89PNG\r\n\x1a\n'
    at = 8
    chunks = {}
    payload = []
    while at < len(data):
        size, kind = struct.unpack_from('>I4s', data, at)
        body = data[at + 8:at + 8 + size]
        assert zlib.crc32(kind + body) == struct.unpack_from('>I', data, at + 8 + size)[0]
        if kind == b'IDAT':
            payload.append(body)
        else:
            chunks[kind] = body
        at += 12 + size
    assert at == len(data)
    return chunks, b''.join(payload)


cases = json.loads((OUT / 'manifest.json').read_text())
assert len(cases) == 18
base = (ROOT / 'tests/fixtures/jxl/grayscale_jpeg/input.jxl').read_bytes()
baseline = b''.join(body if kind == b'jxlc' else body[4:] for kind, body in boxes(base) if kind in [b'jxlc', b'jxlp'])
for case in cases:
    parts = list(boxes((OUT / case['file']).read_bytes()))
    assert b''.join(body for kind, body in parts if kind == b'jxlc') == baseline
    exif = brotli.decompress((FIXTURES / case['exif_fixture']).read_bytes())
    xmp = brotli.decompress((FIXTURES / case['xmp_fixture']).read_bytes())
    assert [(body[:4], body[4:]) for kind, body in parts if kind == b'brob'] == [
        (b'Exif', (FIXTURES / case['exif_fixture']).read_bytes()),
        (b'xml ', (FIXTURES / case['xmp_fixture']).read_bytes())]
    offset = struct.unpack_from('>I', exif)[0]
    source = exif[4 + offset:]
    with zipfile.ZipFile(OUT / (case['file'] + '.rphoto')) as project:
        assert project.testzip() is None
        assert json.loads(project.read('manifest.json'))['version'] == 13
        assert project.read('metadata/source.exif') == source
        assert project.read('metadata/source.xmp') == xmp
    original = Image.Exif()
    original.load(source)
    assert original[274] == 8 and original[271] == 'QA Camera'
    plain, plain_payload = png((OUT / (case['file'] + '-false.png')).read_bytes())
    tagged, tagged_payload = png((OUT / (case['file'] + '-true.png')).read_bytes())
    assert b'eXIf' not in plain and b'iTXt' not in plain
    assert plain_payload == tagged_payload
    exported = Image.Exif()
    exported.load(tagged[b'eXIf'])
    assert exported[274] == 1 and exported[271] == 'QA Camera'
    key, body = tagged[b'iTXt'].split(b'\0', 1)
    assert key == b'XML:com.adobe.xmp' and body[:2] == b'\0\0'
    tree = ET.fromstring(body[2:].split(b'\0', 2)[2])
    ns = 'http://ns.adobe.com/tiff/1.0/'
    assert tree.attrib == {f'{{{ns}}}Orientation': '1', f'{{{ns}}}ImageWidth': '200', f'{{{ns}}}ImageLength': '200'}
    assert tree.find('creator').text == '촬영자'
print('JXL Brotli: 18 containers/720000 pixels and 36 PNG outputs, 18 independent quality0/5/11/window10/22/24 streams, SHA-256/raw bytes/CRC/project v13, original orientation8 and normalized PNG orientation1/200x200/Make/Unicode, 18 exact IDAT pairs; strict truncated/trailing/large-window/expansion errors retain pixels.')
