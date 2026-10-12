"""Independent formula/asset checks for the standalone adjustment HDR fixture.
Uses Python struct/zlib and Pillow; does not call the Rust file reader/renderers.
"""
from pathlib import Path
from zipfile import ZipFile
from io import BytesIO
import json
from PIL import Image
from verify_psd_outputs import Reader
from verify_compressed_psd_outputs import decode

root = Path(__file__).resolve().parents[2] / 'qa-output/adjustment-layer'
with ZipFile(root / 'selected-hdr.rphoto') as project:
    manifest = json.loads(project.read('manifest.json'))
    assert manifest['version'] == 8
    layer = manifest['layers'][1]
    assert layer['adjustment_layer'] is True and not layer['group']
    scope = layer['adjustments']['scope']
    assert scope['mapping'] == [1, 0, 0, 0, 1, 0, 0, 0, 1]
    assert scope['extent'] == [12, 8] and not scope['inverted']
    selection = Image.open(BytesIO(project.read(f"layers/{layer['id']}-adjustment-selection.png")))
    mask = Image.open(BytesIO(project.read(f"layers/{layer['id']}-mask.png")))
    assert selection.mode == mask.mode == 'L'
    assert list(selection.size) == scope['mask_size']
    assert mask.size == (12, 8)
    exposure = 2 ** layer['adjustments']['exposure']
    expected = []
    for y in range(8):
        for x in range(12):
            sx, sy = x - scope['min'][0], y - scope['min'][1]
            coverage = selection.getpixel((sx, sy)) / 255 if 0 <= sx < selection.width and 0 <= sy < selection.height else 0
            weight = mask.getpixel((x, y)) / 255 * layer['opacity']
            factor = 1 + (exposure - 1) * coverage * weight
            alpha = [0, 0.3, 0.7, 1][x % 4]
            colors = [1.25 + x / 10, -0.15 + y / 15, 0.375] if alpha else [0, 0, 0]
            expected.append([c * factor for c in colors] + [alpha])

for extension in ['psd', 'psb']:
    path = root / f'selected-hdr.{extension}'
    reader = Reader(path.read_bytes())
    assert reader.take(4) == b'8BPS'
    large = reader.number('H') == 2
    assert large == (extension == 'psb')
    reader.take(6)
    channels, height, width, depth, mode = [reader.number(f) for f in ['H', 'I', 'I', 'H', 'H']]
    assert (channels, height, width, depth, mode) == (4, 8, 12, 32, 3)
    reader.section()
    reader.section()
    outer = reader.section(large)
    assert not len(outer.section(large).data)
    outer.section()
    assert outer.take(8) == b'8BIMLr32'
    native = outer.section(large)
    assert abs(native.number('h')) == 1
    merged = decode(reader.take(len(reader.data) - reader.at), width, height * channels, depth, large, 0)
    for c in range(channels):
        for pixel, target in enumerate(expected):
            actual = merged[c * width * height + pixel]
            stored=target[c]*target[3]+(1-target[3]) if c<3 else target[c]
            assert abs(actual - stored) < 2e-6, (extension, pixel, c, actual, target[c])
    print(f'{path.name}: independent exposure/feather/mask/HDR/negative RGB/alpha formula and single native layer verified')
