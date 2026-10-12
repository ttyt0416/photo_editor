"""NumPy IEEE binary64 samples encoded independently with tifffile."""
from pathlib import Path
import hashlib
import itertools
import json
import numpy as np
import tifffile

ROOT = Path(__file__).resolve().parent / 'tiff-double'
ROOT.mkdir(exist_ok=True)
structures = [(channels, white, associated) for channels in (1, 2, 3, 4)
              for white in ((False, True) if channels < 3 else (False,))
              for associated in ((False, True) if channels in (2, 4) else (False,))]
values = np.array([-2, -.25, -0., 0., 2**-149, 2**-126, .125, .5, 1, 2, 16, float(np.finfo(np.float32).max),
                   .1, 1.0000000000000002, 2**-150, 2**-151, 1e-300] * 3, dtype='f8').reshape(3, 17)
alpha = np.array([0., -0., 2**-14, .125, 1/3, 1.] * 9, dtype='f8')[:51].reshape(3, 17)
cases = []
for index, (structure, profile, planar, big, endian, compressed) in enumerate(itertools.product(
        structures, ('none', 'working', 'linear'), (False, True), (False, True), ('<', '>'), (False, True))):
    channels, white, associated = structure
    gray = channels < 3
    native = values.copy()
    if profile == 'linear' and not gray:
        native[native > 1e20] = 1e8
    color = np.stack([native] if gray else [native, np.roll(native, 3, 1), np.roll(native, 7, 1)], axis=2)
    if white:
        color = (1. - color.astype('f8')).astype('f8')
    if associated:
        color = (color.astype('f8') * alpha[:, :, None].astype('f8')).astype('f8')
    stored = np.concatenate([color, alpha[:, :, None]], axis=2) if channels in (2, 4) else color
    if channels == 1:
        stored = stored[:, :, 0]
    elif planar:
        stored = np.moveaxis(stored, 2, 0)
    name = f'c{channels}-w{int(white)}-a{int(associated)}-{profile}-p{int(planar)}-b{int(big)}-e{int(endian == ">")}-z{int(compressed)}.tiff'
    orientation = (1, 6, 8)[index % 3]
    xmp = f'<half-tiff orientation="{orientation}">음수 HDR</half-tiff>'.encode()
    tags = [(274, 'H', 1, orientation, False), (700, 'B', len(xmp), xmp, False)]
    if profile != 'none':
        source = (ROOT.parent/'tiff-gray-float'/f'{profile}.icc') if gray else (
            ROOT.parents[2]/'data/color/srgb.icc' if profile == 'working' else ROOT.parent/'tiff-rgb-associated/matrix.icc')
        data = source.read_bytes()
        tags.append((34675, 'B', len(data), data, False))
    tifffile.imwrite(ROOT/name, stored, photometric='miniswhite' if white else ('minisblack' if gray else 'rgb'),
                     extrasamples=('ASSOCALPHA' if associated else 'UNASSALPHA') if channels in (2, 4) else None,
                     planarconfig='separate' if planar and channels > 1 else 'contig',
                     bigtiff=big, byteorder=endian, compression='deflate' if compressed else None,
                     rowsperstrip=1, metadata=None, extratags=tags)
    cases.append(dict(file=name, channels=channels, white=white, associated=associated, profile=profile,
                      planar=planar, big=big, endian=endian, compressed=compressed, orientation=orientation,
                      sha256=hashlib.sha256((ROOT/name).read_bytes()).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases, indent=2)+'\n', encoding='utf-8')
print(f'{len(cases)} independently encoded binary64 TIFF inputs/{len(cases)*51} pixels')
