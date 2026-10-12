"""Independent Gaussian base and per-tone luminance/detail arithmetic."""
from pathlib import Path
import json
import numpy as np
from float_filter_math import gaussian,LIMIT
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/tonal-hdr';m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));pixels=0
for case in m['cases']:
    w,h=case['size'];source=np.asarray(case['source'],dtype='f4').reshape(h,w,4).astype('f8');tone=case['tone'];base=gaussian(source,tone['radius']);lum=source[:,:,:3]@np.array([.2126,.7152,.0722]);low=base[:,:,:3]@np.array([.2126,.7152,.0722]);position=lum.clip(0,1);shadow=np.maximum(1-2*position,0);high=np.maximum(2*position-1,0);weights=np.stack([shadow,1-shadow-high,high],axis=-1);gain=float(np.float32(tone['amount']))/100*(weights@np.asarray(tone['strengths'],dtype='f4').astype('f8'))/100;detail=(lum-low)*gain
    expected=source.copy();active=source[:,:,3]>0;expected[active,:3]=(expected[active,:3]+detail[active,None]).clip(-LIMIT,LIMIT);expected=expected.astype('f4');actual=np.asarray(case['pixels'],dtype='f4').reshape(h,w,4);assert np.isfinite(actual).all();scale=np.maximum(1,np.max(np.abs(source[:,:,:3]),axis=(0,1)))
    # Equal shifts mix channel magnitudes; normalize all RGB by the largest input channel.
    scale=max(scale)
    np.testing.assert_allclose(actual[:,:,:3]/scale,expected[:,:,:3]/scale,rtol=6e-6,atol=max(8e-13,3e-5/scale),err_msg=str(tone))
    np.testing.assert_array_equal(actual[:,:,3],source[:,:,3].astype('f4'))
    pixels+=w*h
print(f"Tonal HDR: {len(m['cases'])} independent SciPy Gaussian/luminance/three-tone gain conditions / {pixels} float pixels; signed strengths, extrema/tiny alpha/hidden RGB/finite output verified")
