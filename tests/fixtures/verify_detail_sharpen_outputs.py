"""Independent colorsys hue + NumPy circular interpolation/luminosity/selection, native PSD planes."""
from pathlib import Path
import json,colorsys
import numpy as np
from psd_tools import PSDImage
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/detail-sharpen'
CENTERS=np.array([0.,30.,60.,120.,180.,240.,270.,300.,360.]);LUMA=np.array([.2126,.7152,.0722]);MAX=np.finfo('f4').max

def transform(source,state):
    source=np.asarray(source,dtype='f4');out=source.copy()
    amount=float(np.float32(state['amount']))
    if amount==0:return out
    sigma=float(np.float32(state['radius']));radius=int(np.ceil(np.float32(state['radius'])*np.float32(3)));kernel=np.exp(-np.arange(-radius,radius+1,dtype='f8')**2/(2*sigma*sigma))
    ysource=source[:,:,:3].astype('f8')@LUMA;alpha=np.clip(source[:,:,3].astype('f8'),0,1);alpha[source[:,:,3]<=0]=0
    numerator=ysource*alpha;denominator=alpha
    for axis in [1,0]:
        def convolve(values):
            result=np.zeros_like(values)
            for y,x in np.ndindex(values.shape):
                for i,weight in enumerate(kernel):
                    nx=x+i-radius if axis==1 else x;ny=y+i-radius if axis==0 else y
                    if 0<=nx<values.shape[1] and 0<=ny<values.shape[0]:result[y,x]+=values[ny,nx]*weight
            return result
        numerator=convolve(numerator);denominator=convolve(denominator)
    base=np.divide(numerator,denominator,out=ysource.copy(),where=denominator>0)
    threshold=float(np.float32(state['threshold']));masking=float(np.float32(state['masking']))
    for y,x in np.ndindex(ysource.shape):
        if source[y,x,3]<=0:continue
        detail=ysource[y,x]-base[y,x];detail=np.sign(detail)*max(abs(detail)-threshold,0);gain=amount
        if masking>0:
            edge=0
            for dx,dy in [(-1,0),(1,0),(0,-1),(0,1)]:
                nx=x+dx;ny=y+dy
                if 0<=nx<source.shape[1] and 0<=ny<source.shape[0] and source[ny,nx,3]>0:edge=max(edge,abs(ysource[ny,nx]-ysource[y,x]))
            t=np.clip(edge/(masking*.25),0,1);gain*=t*t*(3-2*t)
        delta=detail*gain
        if delta!=0:out[y,x,:3]=np.clip(source[y,x,:3].astype('f8')+delta,-MAX,MAX).astype('f4')
    return out

manifest=json.loads((OUT/'formula.json').read_text());width,height=manifest['size'];source=np.fromfile(OUT/'formula-source.rgba32.le',dtype='<f4').reshape(height,width,4)
for i,state in enumerate(manifest['states']):
    actual=np.array(manifest['outputs'][i],dtype='f4').reshape(source.shape);target=transform(source,state);np.testing.assert_allclose(actual,target,rtol=4e-6,atol=2e-6,err_msg=f'state {i}')
    np.testing.assert_array_equal(actual[:,:,3].view('u4'),source[:,:,3].view('u4'))
    np.testing.assert_array_equal(actual[source[:,:,3]==0].view('u4'),source[source[:,:,3]==0].view('u4'))
source=np.fromfile(OUT/'source.rgba32.le',dtype='<f4').reshape(10,12,4);actual=np.fromfile(OUT/'scoped.rgba32.le',dtype='<f4').reshape(source.shape);state=json.loads((OUT/'scoped.json').read_text());changed=transform(source,state);y,x=np.indices((10,12));mask=np.minimum((x+y)*13,255)/255;target=source.copy();target[:,:,:3]=source[:,:,:3]*(1-mask[:,:,None])+changed[:,:,:3]*mask[:,:,None];np.testing.assert_allclose(actual,target,rtol=4e-6,atol=2e-6);np.testing.assert_array_equal(actual[:,:,3].view('u4'),source[:,:,3].view('u4'));np.testing.assert_array_equal(actual[0,0].view('u4'),source[0,0].view('u4'))
for ext in ['psd','psb']:
    psd=PSDImage.open(OUT/f'scoped.{ext}');assert psd.depth==32 and len(psd)==1;np.testing.assert_allclose(psd[0].numpy(),target,rtol=4e-6,atol=2e-6)
print(f'Detail sharpen: {width*height*len(manifest["states"])} independent NumPy filtered pixels, Gaussian luminosity detail/threshold/edge mask, alpha/hidden pixels/HDR/negative, scoped RAW and native PSD/PSB match')
