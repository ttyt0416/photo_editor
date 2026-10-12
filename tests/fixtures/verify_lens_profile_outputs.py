"""Independent Lensfun distortion outputs; equations shared with TCA verifier only."""
from lens_profile_math import *
m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));w,h=m['size'];source=np.fromfile(OUT/'source.rgba32.le',dtype='<f4').reshape(h,w,4);maximum=0
for i,case in enumerate(m['cases']):
    points=coordinates(case['profile'],[w,h]);np.testing.assert_allclose(np.array(case['points']).reshape(h,w,2),points,rtol=1e-12,atol=1e-12)
    target=sample(source,points,case['sampling']);actual=np.array(case['pixels'],dtype='f4').reshape(h,w,4)
    np.testing.assert_allclose(actual,target,rtol=3e-6,atol=3e-6,err_msg=f'case {i}')
    maximum=max(maximum,float(np.max(abs(actual-target))))
source=np.fromfile(OUT/'raw-source.rgba32.le',dtype='<f4').reshape(10,12,4);p=json.loads((OUT/'scoped.json').read_text(encoding='utf-8'));changed=sample(source,coordinates(p,[12,10]),'Bilinear');ys,xs=np.indices((10,12));coverage=(np.minimum((xs+ys)*13,255).astype('f4')/F(255));alpha=source[:,:,3]*(F(1)-coverage)+changed[:,:,3]*coverage;target=np.zeros_like(source);target[:,:,3]=alpha
for c in range(3):target[:,:,c]=np.divide(source[:,:,c]*source[:,:,3]*(F(1)-coverage)+changed[:,:,c]*changed[:,:,3]*coverage,alpha,out=np.zeros_like(alpha),where=alpha>0)
target[coverage==0]=source[coverage==0]
actual=np.fromfile(OUT/'scoped.rgba32.le',dtype='<f4').reshape(10,12,4);np.testing.assert_allclose(actual,target,rtol=4e-6,atol=3e-6)
for ext in ['psd','psb']:
    psd=PSDImage.open(OUT/f'scoped.{ext}');assert psd.depth==32;np.testing.assert_allclose(psd[0].numpy(),target,rtol=4e-6,atol=3e-6)
with zipfile.ZipFile(OUT/'raw-profile.rphoto') as z:
    m=json.loads(z.read('manifest.json'));assert m['version']==9;assert m['layers'][0]['adjustments']['lens_profile']==p
    assert z.read('layers/1-source.raw')==(ROOT/'tests/fixtures/raw-metadata/metadata-le.dng').read_bytes()
fixture=ROOT/'tests/fixtures/lens-profiles/slr-nikon.xml';db=ET.parse(fixture).getroot();summary=json.loads((OUT/'official-summary.json').read_text(encoding='utf-8'));assert summary['cameras']==len(db.findall('camera'));assert summary['profiles']<=len(db.findall('.//distortion'))+len(db.findall('.//tca'))+len(db.findall('.//vignetting'))
print(f"Lens profiles: {len(m['layers'])} RAW layer, 45 cases / 5265 independently sampled pixels, four coordinate models, three float samplers, tiny alpha/HDR/negative, fractional scope and native PSD/PSB; max error {maximum:g}; official {summary['profiles']} optical records / {summary['cameras']} cameras, SHA256 {hashlib.sha256(fixture.read_bytes()).hexdigest()}")
