"""Independent line sampling and SciPy Gaussian/unsharp-mask arithmetic."""
from pathlib import Path
import json
import numpy as np
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/spatial-hdr';LIMIT=float(np.finfo('f4').max)

from float_filter_math import gaussian

def round_away(v):return int(np.copysign(np.floor(abs(float(v))+.5),v))

def motion(source,length,angle):
    if length<=1:return source.copy()
    rad=np.float32(np.float32(angle)*np.float32(np.pi/180));sin=np.float32(np.sin(rad));cos=np.float32(np.cos(rad));h,w,_=source.shape
    result=np.zeros_like(source)
    for y in range(h):
        for x in range(w):
            taps=[]
            for n in range(length):
                t=np.float32(np.float32(n)-np.float32(np.float32(length-1)*np.float32(.5)))
                xx=round_away(np.float32(np.float32(x)+np.float32(t*cos)));yy=round_away(np.float32(np.float32(y)+np.float32(t*sin)))
                if 0<=xx<w and 0<=yy<h:taps.append(source[yy,xx])
            if taps:
                taps=np.asarray(taps);alpha=taps[:,3].sum()
                if alpha>0:
                    result[y,x,:3]=(taps[:,:3]*taps[:,3,None]).sum(axis=0)/alpha
                    result[y,x,3]=alpha/length
    result[:,:,:3]=result[:,:,:3].clip(-LIMIT,LIMIT)
    return result

m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));pixels=0
for case in m['cases']:
    w,h=case['size'];source=np.asarray(case['source'],dtype='f4').reshape(h,w,4).astype('f8')
    if 'motion' in case:expected=motion(source,int(case['motion'][0]),case['motion'][1])
    else:
        low=gaussian(source,1);expected=source.copy();expected[:,:,:3]=(source[:,:,:3]+(source[:,:,:3]-low[:,:,:3])*float(np.float32(case['sharpen']))).clip(-LIMIT,LIMIT)
    actual=np.asarray(case['pixels'],dtype='f4').reshape(h,w,4);assert np.isfinite(actual).all();expected=expected.astype('f4');scale=np.maximum(1,np.max(np.abs(source[:,:,:3]),axis=(0,1)))
    for c in range(3):np.testing.assert_allclose(actual[:,:,c]/scale[c],expected[:,:,c]/scale[c],rtol=4e-6,atol=max(8e-13,2e-5/scale[c]),err_msg=str({k:v for k,v in case.items() if k not in ['source','pixels']}))
    np.testing.assert_allclose(actual[:,:,3],expected[:,:,3],rtol=4e-6,atol=float(np.nextafter(np.float32(0),np.float32(1)))*2)
    pixels+=w*h
print(f"Spatial HDR: {len(m['cases'])} independent line sampling/SciPy Gaussian-unsharp conditions / {pixels} float pixels; extrema/negative/tiny alpha/hidden RGB/edge extension and finite output verified")
