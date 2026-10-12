"""Independent three-coordinate Shepard interpolation on measured Lensfun XML."""
from lens_vignetting_math import *
import xml.etree.ElementTree as ET
OUT=ROOT/'qa-output/lens-vignette-interpolation'
def expected(p,query):
    lens=ET.fromstring(p['xml']);cal=lens.findall('calibration')[p['calibration']];chosen=p['vignetting'][p['vignetting_selected']];model=chosen['model'];rows=[r for r in cal.findall('vignetting') if r.get('model')==model]
    nominal=lens.find('focal');focals=[float(r.get('focal')) for r in cal if r.get('focal')]
    span=float(nominal.get('max',nominal.text))-float(nominal.get('min',nominal.text)) if nominal is not None else max(focals)-min(focals)
    x=np.array([[float(r.get('focal')),float(r.get('aperture')),float(r.get('distance'))] for r in rows]);q=np.array(query,dtype='f8');delta=x-q
    if span>0:delta[:,0]/=span
    delta[:,1]=4/x[:,1]-4/q[1];delta[:,2]=.1/x[:,2]-.1/q[2];distance=np.linalg.norm(delta,axis=1)
    keys=['k1','k2','k3'] if model=='pa' else ['alpha1','alpha2','alpha3'];coefficients=np.array([[float(r.get(key,'0')) for key in keys] for r in rows]);near=np.flatnonzero(distance<1e-4)
    if len(near):return coefficients[near[0]]
    weights=distance**(-3.5);weights/=np.sum(weights)
    if model=='acm':coefficients*=x[:,0,None]**(-2*np.arange(1,4));return (weights@coefficients)*q[0]**(2*np.arange(1,4))
    return weights@coefficients
cases=json.loads((OUT/'profiles.json').read_text(encoding='utf-8'))
for case in cases:
    p=case['profile'];np.testing.assert_allclose(p['vignetting'][p['vignetting_selected']]['coefficients'],expected(p,case['query']),rtol=1e-12,atol=1e-14)
source=np.fromfile(OUT/'raw-source.rgba32.le',dtype='<f4').reshape(10,12,4);p=json.loads((OUT/'scoped.json').read_text(encoding='utf-8'));state=json.loads(json.dumps(p));state['vignetting'][state['vignetting_selected']]['coefficients']=expected(p,p['vignetting_query']).tolist();changed,_=transform(correct(source,state),state,'Bilinear');ys,xs=np.indices((10,12));coverage=np.minimum((xs+ys)*13,255).astype('f4')/F(255);alpha=source[:,:,3]*(F(1)-coverage)+changed[:,:,3]*coverage;target=np.zeros_like(source);target[:,:,3]=alpha
for c in range(3):target[:,:,c]=np.divide(source[:,:,c]*source[:,:,3]*(F(1)-coverage)+changed[:,:,c]*changed[:,:,3]*coverage,alpha,out=np.zeros_like(alpha),where=alpha>0)
target[coverage==0]=source[coverage==0];actual=np.fromfile(OUT/'scoped.rgba32.le',dtype='<f4').reshape(10,12,4);np.testing.assert_allclose(actual,target,rtol=4e-6,atol=3e-6)
for ext in ['psd','psb']:
    psd=PSDImage.open(OUT/f'scoped.{ext}');assert psd.depth==32;np.testing.assert_allclose(psd[0].numpy(),target,rtol=4e-6,atol=3e-6)
with zipfile.ZipFile(OUT/'raw-profile.rphoto') as z:
    record=json.loads(z.read('manifest.json'));assert record['layers'][0]['adjustments']['lens_profile']==p;assert z.read('layers/1-source.raw')==(ROOT/'tests/fixtures/raw-metadata/metadata-le.dng').read_bytes()
print(f"Lens multidimensional vignetting: {len(cases)} independent XML/Shepard solutions, PA/ACM focal-aperture-distance axes, inverse distance power 3.5, exact/near/single focal and coefficient scaling; scoped RAW pixels/source assets and native PSD/PSB verified")
