"""Independent ZIP recipe/RAW preservation check; pixel algorithms have separate QA."""
from pathlib import Path
import zipfile,json,hashlib
ROOT=Path(__file__).resolve().parents[2]
original=(ROOT/'tests/fixtures/raw/linear-32-o1.dng').read_bytes()
with zipfile.ZipFile(ROOT/'qa-output/raw-advanced/recipe.rphoto') as z:
    assert z.testzip() is None
    m=json.loads(z.read('manifest.json'));assert m['version']==9
    layer=m['layers'][0];a=layer['adjustments']
    assert layer['bit_depth']==32 and layer['offset']==[2,3]
    assert layer['transform']['angle']==7 and layer['mask'] is not None
    assert a['contrast']==.2 and a['curves'][0]['points']==[[0,0],[.5,.7],[1,1]]
    assert a['color_mixer']['hue'][2]==18 and a['color_grading']['tones'][0]['saturation']==.2
    assert a['denoise']['luminance']==.1 and a['detail_sharpen']['amount']==.2 and a['lens']['zoom']==1.02
    assert z.read('layers/1-source.raw')==original
print('RAW advanced: independent ZIP CRC/v9 recipe, curve/mixer/grading/detail/lens, 32-bit asset/offset/angle/mask and exact synthetic source SHA-256 '+hashlib.sha256(original).hexdigest())
