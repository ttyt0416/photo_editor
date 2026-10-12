"""Separate XML parsing and cubic boundary-condition solve for measured focal interpolation."""
from lens_tca_math import *
import xml.etree.ElementTree as ET
OUT=ROOT/'qa-output/lens-interpolation'
def interpolate(rows,target,value,power):
    rows=sorted(rows,key=lambda r:float(r.get('focal')));focals=np.array([float(r.get('focal')) for r in rows]);index=np.flatnonzero(abs(focals-target)<1e-6)
    if len(index):return value(rows[index[0]])
    right=np.searchsorted(focals,target);assert 0<right<len(rows);left=right-1;t=(target-focals[left])/(focals[right]-focals[left]);a=value(rows[left])*focals[left]**power;b=value(rows[right])*focals[right]**power
    dl=b-a if left==0 else (b-value(rows[left-1])*focals[left-1]**power)/2
    dr=b-a if right+1==len(rows) else (value(rows[right+1])*focals[right+1]**power-a)/2
    polynomial=np.linalg.solve(np.array([[1,0,0,0],[1,1,1,1],[0,1,0,0],[0,1,2,3]],dtype='f8'),np.array([a,b,dl,dr]));return np.polynomial.polynomial.polyval(t,polynomial)/target**power

def expected(profile):
    lens=ET.fromstring(profile['xml']);cal=lens.findall('calibration')[profile['calibration']];target=profile['focal'];model=profile['model'];rows=[r for r in cal.findall('distortion') if r.get('model')==model];keys={'poly3':['k1'],'poly5':['k1','k2'],'ptlens':['a','b','c'],'acm':['k1','k2','k3','k4','k5']}[model]
    coefficients=[]
    for i in range(5):
        power=1-2*(i+1) if model=='acm' and i<3 else 0 if model=='acm' else 1
        coefficients.append(interpolate(rows,target,lambda r:float(r.get(keys[i],'0')) if i<len(keys) else 0.,power))
    real=interpolate(rows,target,lambda r:float(r.get('real-focal',r.get('focal'))),0)
    rows=cal.findall('tca');tmodel=profile['tca']['model'];channels=[]
    for blue in [False,True]:
        if tmodel=='linear':keys=['kb' if blue else 'kr']
        elif tmodel=='poly3':keys=['vb','cb','bb'] if blue else ['vr','cr','br']
        else:keys=[('beta' if blue else 'alpha')+str(i) for i in range(6)]
        channel=[]
        for i in range(6):
            power=1-2*i if tmodel=='acm' and 1<=i<=3 else 0 if tmodel=='acm' or i==0 else 1
            channel.append(interpolate(rows,target,lambda r:float(r.get(keys[i],'1' if i==0 else '0')) if i<len(keys) else 0.,power))
        channels.append(channel)
    return coefficients,real,channels
profiles=json.loads((OUT/'profiles.json').read_text(encoding='utf-8'))
for case in profiles:
    p=case['profile'];coefficients,real,channels=expected(p);np.testing.assert_allclose(p['coefficients'],coefficients,rtol=1e-12,atol=1e-14);np.testing.assert_allclose(p['real_focal'],real,rtol=1e-12);np.testing.assert_allclose([p['tca']['red'],p['tca']['blue']],channels,rtol=1e-12,atol=1e-14)
source=np.fromfile(OUT/'raw-source.rgba32.le',dtype='<f4').reshape(10,12,4);p=json.loads((OUT/'scoped.json').read_text(encoding='utf-8'));coefficients,real,channels=expected(p);target_profile=dict(p);target_profile['coefficients']=coefficients;target_profile['real_focal']=real;target_profile['tca']=dict(p['tca'],red=channels[0],blue=channels[1]);changed,_=transform(source,target_profile,'Bilinear');ys,xs=np.indices((10,12));coverage=np.minimum((xs+ys)*13,255).astype('f4')/F(255);alpha=source[:,:,3]*(F(1)-coverage)+changed[:,:,3]*coverage;target=np.zeros_like(source);target[:,:,3]=alpha
for c in range(3):target[:,:,c]=np.divide(source[:,:,c]*source[:,:,3]*(F(1)-coverage)+changed[:,:,c]*changed[:,:,3]*coverage,alpha,out=np.zeros_like(alpha),where=alpha>0)
target[coverage==0]=source[coverage==0];actual=np.fromfile(OUT/'scoped.rgba32.le',dtype='<f4').reshape(10,12,4);np.testing.assert_allclose(actual,target,rtol=4e-6,atol=3e-6)
for ext in ['psd','psb']:
    psd=PSDImage.open(OUT/f'scoped.{ext}');assert psd.depth==32;np.testing.assert_allclose(psd[0].numpy(),target,rtol=4e-6,atol=3e-6)
with zipfile.ZipFile(OUT/'raw-profile.rphoto') as z:
    record=json.loads(z.read('manifest.json'));assert record['layers'][0]['adjustments']['lens_profile']==p;assert z.read('layers/1-source.raw')==(ROOT/'tests/fixtures/raw-metadata/metadata-le.dng').read_bytes()
print(f"Lens focal interpolation: {len(profiles)} independent XML/cubic matrix solutions, four distortion and three TCA models, scaled coefficients/real focal/outer tangents/boundaries; RAW fractional scope and native PSD/PSB/source assets verified")
