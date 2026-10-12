"""Independent bright-pass, Gaussian and glow/coverage arithmetic."""
from pathlib import Path
import json
import numpy as np
from float_filter_math import gaussian,LIMIT
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/bloom-hdr';m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));pixels=0

def bloom(source,settings,expand):
    amount,radius,threshold,knee=[float(np.float32(settings[k])) for k in ['amount','radius','threshold','knee']]
    rgb=np.maximum(source[:,:,:3],0);lum=rgb@np.array([.2126,.7152,.0722]);knee*=threshold
    soft=np.clip(lum-threshold+knee,0,2*knee)**2/(4*knee) if knee>0 else np.zeros_like(lum)
    weight=np.maximum(np.maximum(lum-threshold,soft),0)/np.maximum(lum,1e-6)
    bright=source.copy()
    if expand:bright[:,:,:3]=rgb;bright[:,:,3]=np.clip(source[:,:,3]*weight,0,1)
    else:bright[:,:,:3]=rgb*weight[:,:,None]
    bright=bright.astype('f4').astype('f8');blurred=gaussian(bright,radius) if radius>0 else bright
    output=source.copy()
    if expand:
        glow_alpha=blurred[:,:,3]*amount;alpha=source[:,:,3]+(1-source[:,:,3])*glow_alpha.clip(0,1)
        numerator=source[:,:,:3]*source[:,:,3,None]+blurred[:,:,:3]*glow_alpha[:,:,None]
        output[:,:,:3]=0;np.divide(numerator,alpha[:,:,None],out=output[:,:,:3],where=alpha[:,:,None]>0);output[:,:,3]=alpha.clip(0,1)
    else:
        active=source[:,:,3]>0
        output[active,:3]+=blurred[active,:3]*blurred[active,3,None]*amount
    output[:,:,:3]=output[:,:,:3].clip(-LIMIT,LIMIT)
    return output.astype('f4')

for case in m['cases']:
    w,h=case['size'];source=np.asarray(case['source'],dtype='f4').reshape(h,w,4).astype('f8');expected=bloom(source,case['bloom'],case['expand']);actual=np.asarray(case['pixels'],dtype='f4').reshape(h,w,4);assert np.isfinite(actual).all();scale=np.maximum(1,np.max(np.abs(source[:,:,:3]),axis=(0,1)))
    for c in range(3):np.testing.assert_allclose(actual[:,:,c]/scale[c],expected[:,:,c]/scale[c],rtol=6e-6,atol=max(8e-13,3e-5/scale[c]),err_msg=str({k:v for k,v in case.items() if k not in ['source','pixels']}))
    np.testing.assert_allclose(actual[:,:,3],expected[:,:,3],rtol=6e-6,atol=float(np.nextafter(np.float32(0),np.float32(1)))*2)
    pixels+=w*h
print(f"Bloom HDR: {len(m['cases'])} independent bright-pass/Gaussian/glow conditions / {pixels} float pixels; finite extrema/negative/tiny alpha, preserved-alpha and expanded-coverage paths verified")
