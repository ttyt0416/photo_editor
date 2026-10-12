"""Fetch pinned public libjxl ICC references, verifying every corpus object hash."""
from pathlib import Path
import hashlib
import io
import json
import urllib.request
import zlib
import numpy as np

OUT = Path(__file__).resolve().parent / 'jxl-icc'
COMMIT = 'b1d0f990b03e57bf6d137c365cd5dc8b470b9191'
BASE = f'https://raw.githubusercontent.com/libjxl/conformance/{COMMIT}/testcases'


def fetch(url):
    with urllib.request.urlopen(url, timeout=30) as response:
        data = response.read(64 * 1024 * 1024 + 1)
    assert len(data) <= 64 * 1024 * 1024
    return data


files = {}
cases = {}
for name in ['bench_oriented_brg', 'grayscale']:
    folder = OUT / name
    folder.mkdir(parents=True, exist_ok=True)
    for item in ['input.jxl', 'test.json']:
        url = f'{BASE}/{name}/{item}'
        data = fetch(url)
        (folder / item).write_bytes(data)
        files[f'{name}/{item}'] = {'url': url, 'size': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
    descriptor = json.loads((folder / 'test.json').read_text())
    for item in ['original.icc', 'reference.icc', 'reference_image.npy']:
        digest = descriptor['sha256sums'][item]
        url = f'https://storage.googleapis.com/jxl-conformance/objects/{digest}'
        data = fetch(url)
        assert hashlib.sha256(data).hexdigest() == digest
        stored = zlib.compress(data, 9) if item.endswith('.npy') else data
        output = item + '.zlib' if item.endswith('.npy') else item
        (folder / output).write_bytes(stored)
        files[f'{name}/{output}'] = {'url': url, 'size': len(stored), 'sha256': hashlib.sha256(stored).hexdigest(),
                                    'original_size': len(data), 'original_sha256': digest}
        if item.endswith('.npy'):
            array = np.load(io.BytesIO(data), allow_pickle=False)
            cases[name] = {'shape': list(array.shape)}
(OUT / 'manifest.json').write_text(json.dumps({'repository': 'https://github.com/libjxl/conformance', 'commit': COMMIT,
    'license': '../jxl/LICENSE', 'source_license': 'CC0; upstream testcases README permits testing reproduction',
    'files': files, 'cases': cases}, indent=2) + '\n', encoding='utf-8')
print(json.dumps({'cases': cases, 'stored_bytes': sum(f['size'] for f in files.values())}))
