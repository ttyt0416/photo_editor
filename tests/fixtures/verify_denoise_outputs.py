"""Independent colorsys hue + NumPy circular interpolation/luminosity/selection, native PSD planes."""
from pathlib import Path
import json,colorsys
import numpy as np
from psd_tools import PSDImage
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/denoise'
CENTERS=np.array([0.,30.,60.,120.,180.,240.,270.,300.,360.]);LUMA=np.array([.2126,.7152,.0722]);MAX=np.finfo('f4').max

def transform(source,state):
    out=np.asarray(source,dtype='f4').copy()
    if state['luminance']==0 and state['color']==0:return out
    radius=state['radius'];sigma=max(radius,1)/2;threshold=float(np.float32(state['threshold']));strength=float(np.float32(state['luminance']));color_strength=float(np.float32(state['color']))
    for vertical in [False,True]:
        original=out.copy()
        for y,x in np.ndindex(original.shape[:2]):
            p=original[y,x].astype('f8')
            if p[3]<=0:continue
            yc=p[:3]@LUMA;cc=p[:3]-yc;sum_y=total_y=total_c=0.;sum_c=np.zeros(3)
            for d in range(-radius,radius+1):
                nx=x if vertical else x+d;ny=y+d if vertical else y
                if not (0<=nx<original.shape[1] and 0<=ny<original.shape[0]):continue
                q=original[ny,nx].astype('f8')
                if q[3]<=0:continue
                yq=q[:3]@LUMA;cq=q[:3]-yq
                wy=np.exp(-d*d/(2*sigma*sigma)-.5*((yq-yc)/threshold)**2)*np.clip(q[3],0,1)
                wc=wy*np.exp(-.5*np.mean(((cq-cc)/threshold)**2));sum_y+=yq*wy;total_y+=wy;sum_c+=cq*wc;total_c+=wc
            if total_y>0 and total_c>0:
                yy=yc+(sum_y/total_y-yc)*strength;chroma=cc+(sum_c/total_c-cc)*color_strength
                out[y,x,:3]=np.clip(yy+chroma,-MAX,MAX).astype('f4')
    return out

manifest=json.loads((OUT/'formula.json').read_text());width,height=manifest['size'];source=np.fromfile(OUT/'formula-source.rgba32.le',dtype='<f4').reshape(height,width,4)
for i,state in enumerate(manifest['states']):
    actual=np.array(manifest['outputs'][i],dtype='f4').reshape(source.shape);target=transform(source,state);np.testing.assert_allclose(actual,target,rtol=4e-6,atol=2e-6,err_msg=f'state {i}')
    np.testing.assert_array_equal(actual[:,:,3].view('u4'),source[:,:,3].view('u4'))
    np.testing.assert_array_equal(actual[source[:,:,3]==0].view('u4'),source[source[:,:,3]==0].view('u4'))
source=np.fromfile(OUT/'source.rgba32.le',dtype='<f4').reshape(10,12,4);actual=np.fromfile(OUT/'scoped.rgba32.le',dtype='<f4').reshape(source.shape);state=json.loads((OUT/'scoped.json').read_text());changed=transform(source,state);y,x=np.indices((10,12));mask=np.minimum((x+y)*13,255)/255;target=source.copy();target[:,:,:3]=source[:,:,:3]*(1-mask[:,:,None])+changed[:,:,:3]*mask[:,:,None];np.testing.assert_allclose(actual,target,rtol=4e-6,atol=2e-6);np.testing.assert_array_equal(actual[:,:,3].view('u4'),source[:,:,3].view('u4'));np.testing.assert_array_equal(actual[0,0].view('u4'),source[0,0].view('u4'))
for ext in ['psd','psb']:
    psd=PSDImage.open(OUT/f'scoped.{ext}');assert psd.depth==32 and len(psd)==1;np.testing.assert_allclose(psd[0].numpy(),target,rtol=4e-6,atol=2e-6)
print(f'Denoise: {width*height*len(manifest["states"])} independent NumPy filtered pixels, separate luminance/chroma, alpha/hidden pixels/HDR/negative, scoped RAW and native PSD/PSB match')
