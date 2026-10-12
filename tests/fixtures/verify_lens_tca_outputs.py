"""Independent TCA output verification."""
from lens_tca_math import *
m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));w,h=m['size'];source=np.fromfile(OUT/'source.rgba32.le',dtype='<f4').reshape(h,w,4);maximum=0
for i,case in enumerate(m['cases']):
    target,points=transform(source,case['profile'],case['sampling']);np.testing.assert_allclose(np.array(case['points']).reshape(h,w,3,2),points,rtol=1e-12,atol=1e-12)
    actual=np.array(case['pixels'],dtype='f4').reshape(h,w,4);np.testing.assert_allclose(actual,target,rtol=3e-6,atol=3e-6,err_msg=f'case {i}');maximum=max(maximum,float(np.max(abs(actual-target))))
source=np.fromfile(OUT/'raw-source.rgba32.le',dtype='<f4').reshape(10,12,4);p=json.loads((OUT/'scoped.json').read_text(encoding='utf-8'));changed,_=transform(source,p,'Bilinear');ys,xs=np.indices((10,12));coverage=np.minimum((xs+ys)*13,255).astype('f4')/F(255);alpha=source[:,:,3]*(F(1)-coverage)+changed[:,:,3]*coverage;target=np.zeros_like(source);target[:,:,3]=alpha
for c in range(3):target[:,:,c]=np.divide(source[:,:,c]*source[:,:,3]*(F(1)-coverage)+changed[:,:,c]*changed[:,:,3]*coverage,alpha,out=np.zeros_like(alpha),where=alpha>0)
target[coverage==0]=source[coverage==0];actual=np.fromfile(OUT/'scoped.rgba32.le',dtype='<f4').reshape(10,12,4);np.testing.assert_allclose(actual,target,rtol=4e-6,atol=3e-6)
for ext in ['psd','psb']:
    psd=PSDImage.open(OUT/f'scoped.{ext}');assert psd.depth==32;np.testing.assert_allclose(psd[0].numpy(),target,rtol=4e-6,atol=3e-6)
with zipfile.ZipFile(OUT/'raw-profile.rphoto') as z:
    record=json.loads(z.read('manifest.json'));assert record['layers'][0]['adjustments']['lens_profile']==p;assert z.read('layers/1-source.raw')==(ROOT/'tests/fixtures/raw-metadata/metadata-le.dng').read_bytes()
summary=json.loads((OUT/'official-summary.json').read_text(encoding='utf-8'));assert summary['tca']>100
print(f"Lens TCA: {len(m['cases'])} cases / {w*h*len(m['cases'])} independent float pixels; linear/poly3/ACM red-blue geometry, three samplers, distortion order, reference green/alpha, tiny alpha/HDR/negative, RAW scope and PSD/PSB; max error {maximum:g}; official {summary['tca']} measured TCA connections / {summary['profiles']} optical records")
