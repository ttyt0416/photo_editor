"""Independent f32 interpolation weights with f64 premultiplied accumulation."""
from lens_profile_math import *
OUT=ROOT/'qa-output/float-sampling';m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));w,h=m['size'];count=0
for case in m['cases']:
    source=np.asarray(case['source'],dtype='f4').reshape(h,w,4);expected=[]
    for point in case['points']:
        grid=np.broadcast_to(np.asarray(point,dtype='f4'),(h,w,2));expected.append(sample(source,grid,case['sampling'])[0,0]);count+=1
    np.testing.assert_allclose(np.asarray(case['pixels'],dtype='f4'),np.asarray(expected),rtol=3e-6,atol=2e-6)
with zipfile.ZipFile(OUT/'tiny-alpha.rphoto') as z:
    record=json.loads(z.read('manifest.json'));assert record['layers'][0]['transform']['sampling']=='Bicubic'
for ext in ['psd','psb']:
    image=PSDImage.open(OUT/f'tiny-alpha.{ext}');assert image.depth==32;pixels=image[0].numpy();assert np.any((pixels[:,:,3]>0)&(pixels[:,:,3]<1e-10));assert np.any(pixels[:,:,:3]>1)
print(f"Common float sampling: {count} independently weighted results / {len(m['cases'])} source-sampler cases; f32 extrema, SDR/negative/HDR, zero/subnormal/tiny alpha, transparent edges and cubic negative lobes; saved transform and native 32-bit PSD/PSB tiny alpha retained")
