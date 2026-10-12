"""Independent SciPy convolution of float64 premultiplied RGBA with clamped borders."""
from pathlib import Path
import json
import numpy as np
from scipy.ndimage import convolve1d
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/float-blur';m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));LIMIT=float(np.finfo('f4').max);pixels=0
for case in m['cases']:
    w,h=case['size']
    sigma=np.float32(case['sigma'])
    extent=np.float32(sigma-np.float32(.8))/np.float32(.3)
    extent=np.float32(np.float32(extent+np.float32(1))*np.float32(2))+np.float32(1)
    size=int(max(extent,np.float32(3)))
    if size%2==0: size+=1
    d=np.arange(size,dtype='f8')-size//2;kernel=np.exp(-.5*(d/float(sigma))**2);kernel/=kernel.sum()
    source=np.asarray(case['source'],dtype='f4').reshape(h,w,4).astype('f8');alpha=source[:,:,3].clip(0,1);prem=source.copy();prem[:,:,:3]*=alpha[:,:,None];prem[:,:,3]=alpha
    changed=convolve1d(convolve1d(prem,kernel,axis=1,mode='nearest'),kernel,axis=0,mode='nearest');target=np.zeros_like(changed);target[:,:,3]=changed[:,:,3].clip(0,1);np.divide(changed[:,:,:3],changed[:,:,3,None],out=target[:,:,:3],where=changed[:,:,3,None]>0);target[:,:,:3]=target[:,:,:3].clip(-LIMIT,LIMIT)
    actual=np.asarray(case['pixels'],dtype='f4').reshape(h,w,4)
    expected=target.astype('f4')
    # Opposing f32 extrema can cancel to zero; compare RGB against input channel scale.
    scale=np.maximum(1,np.max(np.abs(source[:,:,:3]),axis=(0,1)))
    for channel in range(3):
        np.testing.assert_allclose(actual[:,:,channel]/scale[channel],expected[:,:,channel]/scale[channel],rtol=4e-6,atol=max(8e-14,2e-6/scale[channel]),err_msg=f"{w}x{h} sigma{sigma}")
    np.testing.assert_allclose(actual[:,:,3],expected[:,:,3],rtol=4e-6,atol=float(np.nextafter(np.float32(0),np.float32(1)))*2)
    pixels+=w*h
print(f"Float Gaussian blur: {len(m['cases'])} independently computed kernels/SciPy two-axis convolutions / {pixels} float pixels; one-pixel/row/column/2D clamped borders, zero/subnormal/tiny alpha, f32 extrema/HDR/negative; finite output verified")
