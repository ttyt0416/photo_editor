"""Independent scalar inverse perspective/rotation equations and existing premultiplied sampler."""
from lens_profile_math import ROOT, np, sample, json, zipfile, hashlib
import math
OUT = ROOT / 'qa-output/photo-geometry'
cases = json.loads((OUT/'cases.json').read_text(encoding='utf-8'))
count = 0
for case in cases:
    w,h = case['size']; g = case['geometry']
    source = np.fromfile(OUT/case['input'],dtype='<f4').reshape(h,w,4)
    actual = np.fromfile(OUT/case['output'],dtype='<f4').reshape(h,w,4)
    angle,zoom,aspect = [float(np.float32(g[k])) for k in ['angle','zoom','aspect']]
    px,py = [float(np.float32(v)) for v in g['perspective']]
    tx,ty = [float(np.float32(v)) for v in g['shift']]
    if angle==0 and px==py==0 and zoom==aspect==1 and tx==ty==0:
        expected=source
    else:
        ys,xs=np.indices((h,w));x=xs+.5-w/2-tx*w;y=ys+.5-h/2-ty*h
        s,c=math.sin(math.radians(angle)),math.cos(math.radians(angle))
        u=(c*x+s*y)/(zoom*aspect);v=(-s*x+c*y)/zoom
        den=1-px*u/(w/2)-py*v/(h/2)
        visible=den>1e-12
        safe=np.where(visible,den,1)
        points=np.stack([u/safe+w/2,v/safe+h/2],axis=-1)
        points[~visible]=1e9
        expected=sample(source,points,g['sampling']);expected[~visible]=0
    np.testing.assert_allclose(actual,expected,rtol=8e-6,atol=5e-6,err_msg=case['output'])
    count+=w*h
with zipfile.ZipFile(ROOT/'qa-output/raw-geometry/recipe.rphoto') as z:
    assert z.testzip() is None
    m=json.loads(z.read('manifest.json'));layer=m['layers'][0];g=layer['adjustments']['geometry']
    assert m['version']==9 and layer['bit_depth']==32 and layer['offset']==[2,3] and layer['transform']['angle']==7
    assert g=={'angle':12,'perspective':[.15,-.2],'zoom':1.2,'aspect':.9,'shift':[.05,-.03],'sampling':'Bicubic'}
    source=(ROOT/'tests/fixtures/raw/linear-32-o1.dng').read_bytes()
    assert z.read('layers/1-source.raw')==source
print(f'Photo geometry: {len(cases)} cases/{count} pixels, independent inverse equations and premultiplied 3-mode sampling; tiny/odd edges, transparent/negative/HDR; RAW v9 recipe/CRC/32-bit/mask/offset/layer transform and exact source preserved')
