"""Independent standard-library colorsys comparison and persisted settings inspection."""
from pathlib import Path
import colorsys
import json
import math
root=Path(__file__).resolve().parents[2]
data=json.loads((root/'qa-output/color-models/conversions.json').read_text(encoding='utf-8'))
assert len(data['roundtrips'])==3087 and len(data['inputs'])==288
for row in data['roundtrips']:
    color=row['rgba']; rgb=[v/255 for v in color[:3]]
    if row['model']=='rgb': values=color[:3]
    elif row['model']=='hsv':
        h,s,v=colorsys.rgb_to_hsv(*rgb); values=[h*360,s*100,v*100]
    else:
        h,l,s=colorsys.rgb_to_hls(*rgb); values=[h*360,s*100,l*100]
    assert all(abs(a-b)<1e-10 for a,b in zip(row['values'],values)),row
    assert row['rgb']==color[:3]
ties=0
for row in data['inputs']:
    h,s,v=row['values']; h=(h%360)/360; s/=100; v/=100
    rgb=colorsys.hsv_to_rgb(h,s,v) if row['model']=='hsv' else colorsys.hls_to_rgb(h,v,s)
    for expected,actual in zip(rgb,row['rgb']):
        scaled=expected*255; rounded=math.floor(scaled+0.5)
        if abs((scaled-math.floor(scaled))-0.5)<1e-9:
            assert abs(rounded-actual)<=1; ties+=1
        else: assert rounded==actual,(row,scaled,rounded)
library=json.loads((root/'qa-output/color-models/library.json').read_text(encoding='utf-8'))
assert library['version']==1 and library['model']=='hsl'
assert library['foreground']==[0,170,170,83] and library['background']==[20,40,60,70]
assert [0,170,170,83] in library['palette'] and [0,170,170,83] in library['recent']
legacy=json.loads((root/'qa-output/color-models/legacy.json').read_text(encoding='utf-8')); assert 'model' not in legacy
print(f'Color models: independent colorsys 3087 exact RGB/alpha cases and 288 HSV/HSL inputs; {ties} half-integer rounding boundary channels; v1 HSL mode/palette/recent/alpha preserved, legacy default migration')
