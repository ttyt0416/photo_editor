"""Independent Gaussian/tone/selection formula checks for Rust-generated HDR outputs.
Uses Python math/struct and Pillow assets, without Rust readers or renderers.
"""
from pathlib import Path
from zipfile import ZipFile
from io import BytesIO
import json
import math
from PIL import Image
from verify_psd_outputs import Reader
from verify_compressed_psd_outputs import decode

root = Path(__file__).resolve().parents[2] / 'qa-output/tonal-contrast'
with ZipFile(root / 'selected-hdr.rphoto') as project:
    manifest = json.loads(project.read('manifest.json'))
    assert manifest['version'] == 8
    layer = manifest['layers'][0]
    tone = layer['adjustments']['tonal_contrast']
    assert tone == {'amount': 75, 'radius': 2, 'strengths': [80, -30, 60]}
    scope = layer['adjustments']['scope']
    mask = Image.open(BytesIO(project.read(f"layers/{layer['id']}-adjustment-selection.png")))
    assert mask.size == (6, 4)

width, height = 12, 6
source = []
for y in range(height):
    row = []
    for x in range(width):
        v = [0.1, 0.25, 0.4, 0.6, 0.8, 1.8][x % 6] + y * 0.01
        row.append([v, v - 0.15, v + 0.2, 0 if x == 0 else 0.75])
    source.append(row)
# image 0.25.10 uses a symmetric 11-tap Gaussian kernel at sigma 2, with edge replication.
weights = [math.exp(-0.5 * (i / 2) ** 2) for i in range(-5, 6)]
weights = [w / sum(weights) for w in weights]
base = []
for y in range(height):
    row = []
    for x in range(width):
        sums = [0.0] * 4
        for ky, wy in enumerate(weights):
            for kx, wx in enumerate(weights):
                p = source[min(height - 1, max(0, y + ky - 5))][min(width - 1, max(0, x + kx - 5))]
                w = wx * wy
                for c in range(3):
                    sums[c] += p[c] * p[3] * w
                sums[3] += p[3] * w
        row.append([sums[c] / sums[3] if sums[3] else 0 for c in range(3)] + [sums[3]])
    base.append(row)

def lum(p):
    return sum(p[c] * [0.2126, 0.7152, 0.0722][c] for c in range(3))

expected = []
for y in range(height):
    for x in range(width):
        p = source[y][x]
        if not p[3]:
            expected.append([0] * 4)
            continue
        position = min(1, max(0, lum(p)))
        shadows, highlights = max(0, 1 - 2 * position), max(0, 2 * position - 1)
        gain = tone['amount'] / 100 * sum(s * w / 100 for s, w in zip(tone['strengths'], [shadows, 1 - shadows - highlights, highlights]))
        sx, sy = x - scope['min'][0], y - scope['min'][1]
        coverage = mask.getpixel((sx, sy)) / 255 if 0 <= sx < mask.width and 0 <= sy < mask.height else 0
        detail = (lum(p) - lum(base[y][x])) * gain * coverage
        expected.append([v + detail for v in p[:3]] + [p[3]])

for extension in ['psd', 'psb']:
    reader = Reader((root / f'selected-hdr.{extension}').read_bytes())
    assert reader.take(4) == b'8BPS'
    large = reader.number('H') == 2
    assert large == (extension == 'psb')
    reader.take(6)
    channels, h, w, depth, mode = [reader.number(f) for f in ['H', 'I', 'I', 'H', 'H']]
    assert (channels, h, w, depth, mode) == (4, height, width, 32, 3)
    reader.section(); reader.section(); reader.section(large)
    merged = decode(reader.take(len(reader.data) - reader.at), width, height * channels, depth, large, 0)
    for c in range(4):
        for pixel, target in enumerate(expected):
            actual = merged[c * width * height + pixel]
            stored=target[c]*target[3]+(1-target[3]) if c<3 else target[c]
            assert abs(actual - stored) < 3e-6, (extension, pixel, c, actual, target[c])
    print(f'{extension}: independent Gaussian/local tone/feather/HDR/negative RGB/alpha formula verified')
