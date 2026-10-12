"""Create deterministic synthetic Brotli metadata with an independent encoder."""
from pathlib import Path
import brotli
import hashlib
import json
import struct

OUT = Path(__file__).resolve().parent / 'jxl-metadata'
OUT.mkdir(exist_ok=True)
xmp = ("<?xpacket begin='\ufeff'?><r xmlns:t='http://ns.adobe.com/tiff/1.0/' t:Orientation='8' t:ImageWidth='7' t:ImageLength='5'><creator>촬영자</creator><!--" + 'opaque' * 12000 + "--></r><?xpacket end='w'?>").encode()


def exif(little):
    endian = '<' if little else '>'
    return ((b'II' if little else b'MM') + struct.pack(endian + 'HIH', 42, 8, 2)
            + struct.pack(endian + 'HHII', 271, 2, 10, 38)
            + struct.pack(endian + 'HHI', 274, 3, 1) + struct.pack(endian + 'H', 8)
            + b'\0\0' + struct.pack(endian + 'I', 0) + b'QA Camera\0')


cases = []
for quality in [0, 5, 11]:
    for window in [10, 22, 24]:
        little = (quality + window) % 2 == 0
        prefix = b'Exif\0\0' if little else b''
        payload = struct.pack('>I', len(prefix)) + prefix + exif(little)
        entry = {'quality': quality, 'window': window, 'little': little}
        for kind, raw in [('exif', payload), ('xmp', xmp)]:
            name = f'q{quality}-w{window}-{kind}.br'
            encoded = brotli.compress(raw, quality=quality, lgwin=window)
            (OUT / name).write_bytes(encoded)
            entry[kind] = name
            entry[kind + '_sha256'] = hashlib.sha256(encoded).hexdigest()
            entry[kind + '_raw_sha256'] = hashlib.sha256(raw).hexdigest()
            entry[kind + '_size'] = len(raw)
        cases.append(entry)
bomb = brotli.compress(b'x' * (16 * 1024 * 1024 + 5), quality=9, lgwin=24)
(OUT / 'over-budget.br').write_bytes(bomb)
(OUT / 'manifest.json').write_text(json.dumps({'encoder': 'Python Brotli', 'version': brotli.__version__, 'cases': cases,
    'over_budget_sha256': hashlib.sha256(bomb).hexdigest(), 'over_budget_size': 16 * 1024 * 1024 + 5}, indent=2) + '\n', encoding='utf-8')
print(f'JXL synthetic metadata: {len(cases)} combinations/18 Brotli streams plus one expansion above 16MiB.')
