"""Independent colorsys hue + NumPy circular interpolation/luminosity/selection, native PSD planes."""
from pathlib import Path
import json,colorsys
import numpy as np
from psd_tools import PSDImage
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/color-grading'
CENTERS=np.array([0.,30.,60.,120.,180.,240.,270.,300.,360.]);LUMA=np.array([.2126,.7152,.0722]);MAX=np.finfo('f4').max

def transform(c,state):
    c=np.asarray(c,dtype='f4').astype('f8');tones=state['tones']
    if all(t['saturation']==0 and t['luminance']==0 for t in tones):return c.astype('f4')
    y=c@LUMA;t=np.clip(y+float(np.float32(state['balance']))/400,0,1);power=4-float(np.float32(state['blending']))*.03
    w=np.array([(1-t)**power,max(0,1-abs(2*t-1))**power,t**power]);w/=w.sum();color=c-y;bright=0.
    for i,grade in enumerate(tones):
        weight=1 if i==3 else w[i];pure=np.array(colorsys.hsv_to_rgb(float(np.float32(grade['hue']))/360,1,1));color+=(pure-pure@LUMA)*abs(y)*.5*float(np.float32(grade['saturation']))*weight
        if i<3:bright+=float(np.float32(grade['luminance']))*weight
    if np.all((c>=0)&(c<=1)):
        scale=1
        for x in color:
            if x>0:scale=min(scale,(1-y)/x)
            elif x<0:scale=min(scale,-y/x)
        color*=np.clip(scale,0,1)
    return np.clip((y+color)*(1+bright)*(1+float(np.float32(tones[3]['luminance']))),-MAX,MAX).astype('f4')

manifest=json.loads((OUT/'formula.json').read_text());samples=np.array(manifest['samples'],dtype='f4');actual=np.array(manifest['outputs'],dtype='f4').reshape(len(manifest['states']),len(samples),3)
for i,state in enumerate(manifest['states']):
    target=np.array([transform(c,state) for c in samples],dtype='f4');np.testing.assert_allclose(actual[i],target,rtol=5e-6,atol=3e-6,err_msg=f'state {i}')
np.testing.assert_array_equal(actual[0].view('u4'),samples.view('u4'))
source=np.fromfile(OUT/'source.rgba32.le',dtype='<f4').reshape(10,12,4);actual=np.fromfile(OUT/'scoped.rgba32.le',dtype='<f4').reshape(source.shape);state=json.loads((OUT/'scoped.json').read_text());changed=np.array([[transform(p[:3],state) for p in row] for row in source]);y,x=np.indices((10,12));mask=np.minimum((x+y)*13,255)/255;target=source.copy();target[:,:,:3]=source[:,:,:3]*(1-mask[:,:,None])+changed*mask[:,:,None];np.testing.assert_allclose(actual,target,rtol=3e-6,atol=2e-6);np.testing.assert_array_equal(actual[:,:,3].view('u4'),source[:,:,3].view('u4'));np.testing.assert_array_equal(actual[0,0].view('u4'),source[0,0].view('u4'))
for ext in ['psd','psb']:
    psd=PSDImage.open(OUT/f'scoped.{ext}');assert psd.depth==32 and len(psd)==1;planes=psd[0].numpy();np.testing.assert_allclose(planes,target,rtol=3e-6,atol=2e-6)
print(f'Color grading: {len(samples)*len(manifest["states"])} independent colorsys/NumPy samples, tonal weights/balance/blending, HDR/negative/float extremes and default bits; scoped RAW/alpha and native 32-bit PSD/PSB pixels match')
