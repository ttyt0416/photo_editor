"""Independent Normal alpha compositing, stable dissolve and pass-through group weighting."""
from pathlib import Path
import json,zipfile
import numpy as np
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/float-composite';m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));LIMIT=float(np.finfo('f4').max)
def over(s,d,opacity):
    s=np.asarray(s,dtype='f4').astype('f8');d=np.asarray(d,dtype='f4').astype('f8');sa=np.clip(s[3],0,1)*float(np.float32(opacity));da=np.clip(d[3],0,1)
    if sa<=0:return np.asarray([*d[:3],da],dtype='f4')
    if da<=0 or sa>=1:return np.asarray([*s[:3],sa],dtype='f4')
    alpha=sa+da*(1-sa);color=np.clip((sa*s[:3]+(1-sa)*da*d[:3])/alpha,-LIMIT,LIMIT);return np.asarray([*color,alpha],dtype='f4')
for case in m['cases']:np.testing.assert_allclose(np.asarray(case['pixels'],dtype='f4'),over(case['source'],case['target'],case['opacity']),rtol=2e-6,atol=2e-6)
mask=(1<<64)-1;w,h=m['size'];actual=np.asarray(m['dissolve'],dtype='f4').reshape(h,w,4)
for y in range(h):
    for x in range(w):
        hash=m['seed']^((x*0x9e3779b97f4a7c15)&mask)^((y*0xbf58476d1ce4e5b9)&mask);hash=((hash^(hash>>30))*0xbf58476d1ce4e5b9)&mask;hash=((hash^(hash>>27))*0x94d049bb133111eb)&mask;hash^=hash>>31;threshold=((hash&0xffffffff)+.5)/4294967296;source=m['source'].copy();source[3]=1 if threshold<float(np.float32(np.float32(source[3])*np.float32(m['opacity']))) else 0;np.testing.assert_array_equal(actual[y,x],over(source,m['target'],1))
rendered=np.fromfile(OUT/'rendered.rgba32.le',dtype='<f4').reshape(3,5,4);group=np.fromfile(OUT/'grouped.rgba32.le',dtype='<f4').reshape(3,5,4);coverage=float(np.float32(np.float32(.5)*np.float32(128/255)))
for y in range(3):
    for x in range(5):
        a=[LIMIT if x%2==0 else -LIMIT,-LIMIT if y%2==0 else LIMIT,2.125,1];b=[-a[0],-a[1],-.5,.4];expected=over(b,a,.5);np.testing.assert_allclose(rendered[y,x],expected,rtol=3e-6,atol=2e-6);weighted=np.asarray(a,dtype='f8')*(1-coverage)+expected.astype('f8')*coverage;np.testing.assert_allclose(group[y,x],weighted.astype('f4'),rtol=3e-6,atol=2e-6)
with zipfile.ZipFile(OUT/'grouped.rphoto') as z:assert any(layer['group'] and layer['pass_through'] for layer in json.loads(z.read('manifest.json'))['layers'])
print(f"Normal float compositing: {len(m['cases'])} independent alpha/opacity/extreme combinations, {w*h} independently hashed dissolve positions, 15 layer and 15 pass-through group/mask pixels; finite HDR/negative/tiny alpha and saved group verified")
