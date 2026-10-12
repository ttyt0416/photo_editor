"""Independent 110-digit Decimal blend math and source-over coverage.
SDR modes: W3C compositing-1 sections 6/10; extra modes keep the app's existing formula policy.
"""
from pathlib import Path
from decimal import Decimal as D,getcontext
import json
import numpy as np
getcontext().prec=110
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/blend-hdr';LIMIT=D.from_float(float(np.finfo('f4').max));ZERO=D(0);ONE=D(1)
def number(v):return D.from_float(float(np.float32(v)))
def clip(v,low=ZERO,high=ONE):return min(max(v,low),high)
def stored(v):return np.float32(float(clip(v,-LIMIT,LIMIT)))
def lum(c):return sum(x*k for x,k in zip(c,[D('.3'),D('.59'),D('.11')]))
def sat(c):return max(c)-min(c)
def set_sat(c,value):
    c=list(c);lo,mid,hi=sorted(range(3),key=lambda i:c[i])
    if c[hi]>c[lo]:c[mid]=(c[mid]-c[lo])*value/(c[hi]-c[lo]);c[hi]=value
    else:c[mid]=c[hi]=ZERO
    c[lo]=ZERO;return c

def set_lum(c,value):
    value=clip(value);delta=value-lum(c);c=[x+delta for x in c];l=lum(c);n=min(c);x=max(c)
    if n<0:c=[l+(v-l)*l/(l-n) for v in c]
    if x>1:c=[l+(v-l)*(1-l)/(x-l) for v in c]
    return c

def channel(mode,s,d):
    if mode in ['Normal','Dissolve']:return s
    if mode=='Multiply':return s*d
    if mode=='Screen':return s+d-s*d
    if mode=='Overlay':return channel('HardLight',d,s)
    if mode=='Darken':return min(s,d)
    if mode=='Lighten':return max(s,d)
    if mode=='ColorDodge':return ZERO if d<=0 else ONE if s>=1 else min(d/(1-s),ONE)
    if mode=='ColorBurn':return ONE if d>=1 else ZERO if s<=0 else ONE-min((1-d)/s,ONE)
    if mode=='HardLight':return 2*s*d if s<=D('.5') else 1-2*(1-s)*(1-d)
    if mode=='SoftLight':
        if s<=D('.5'):return d-(1-2*s)*d*(1-d)
        g=((16*d-12)*d+4)*d if d<=D('.25') else d.sqrt()
        return d+(2*s-1)*(g-d)
    if mode=='Difference':return abs(d-s)
    if mode=='Exclusion':return d+s-2*d*s
    if mode=='Add':return min(d+s,ONE)
    if mode=='Subtract':return max(d-s,ZERO)
    if mode=='LinearBurn':return max(d+s-1,ZERO)
    if mode=='LinearLight':return clip(d+2*s-1)
    if mode=='VividLight':return channel('ColorBurn',2*s,d) if s<D('.5') else channel('ColorDodge',2*s-1,d)
    if mode=='PinLight':return min(d,2*s) if s<D('.5') else max(d,2*s-1)
    if mode=='HardMix':return ZERO if channel('VividLight',s,d)<D('.5') else ONE
    if mode=='Divide':return ONE if s<=0 else min(d/s,ONE)
    raise AssertionError(mode)

def color(mode,s,d):
    if mode=='DarkerColor':return s if sum(s)<sum(d) else d
    if mode=='LighterColor':return s if sum(s)>sum(d) else d
    if mode=='Hue':return set_lum(set_sat(s,sat(d)),lum(d))
    if mode=='Saturation':return set_lum(set_sat(d,sat(s)),lum(d))
    if mode=='Color':return set_lum(s,lum(d))
    if mode=='Luminosity':return set_lum(d,lum(s))
    return [channel(mode,a,b) for a,b in zip(s,d)]

def over(mode,s,d,opacity):
    sa=clip(s[3])*opacity;da=clip(d[3])
    if sa<=0:return [*d[:3],da]
    if da<=0:return [*s[:3],sa]
    alpha=sa+da*(1-sa);blended=color(mode,s[:3],d[:3])
    return [(sa*(1-da)*s[i]+sa*da*blended[i]+(1-sa)*da*d[i])/alpha for i in range(3)]+[alpha]

m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));cache={}
for case in m['color_cases']:
    s=list(map(number,case['source']));d=list(map(number,case['target']));mode=case['mode'];expected=color(mode,s,d);cache[(mode,tuple(s),tuple(d))]=expected
    np.testing.assert_allclose(np.asarray(case['pixels'],dtype='f4'),np.asarray(list(map(stored,expected))),rtol=3e-6,atol=3e-6,err_msg=str(case))
for case in m['cases']:
    s=list(map(number,case['source']));d=list(map(number,case['target']));mode=case['mode'];opacity=number(case['opacity']);sa=clip(s[3])*opacity;da=clip(d[3])
    if sa<=0:expected=[*d[:3],da]
    elif da<=0:expected=[*s[:3],sa]
    else:
        alpha=sa+da*(1-sa);blend=cache[(mode,tuple(s[:3]),tuple(d[:3]))];expected=[(sa*(1-da)*s[i]+sa*da*blend[i]+(1-sa)*da*d[i])/alpha for i in range(3)]+[alpha]
    actual=np.asarray(case['pixels'],dtype='f4');assert np.isfinite(actual).all();target=np.asarray(list(map(stored,expected)))
    if sa>0 and da>0:
        # Near-cancellation of huge operands cannot retain a tiny residual in f64.
        # Normalize by independently computed contribution magnitude, capped at f32 storage.
        scales=[float(min(LIMIT,max(ONE,abs(sa*(1-da)*s[i]/alpha),abs(sa*da*blend[i]/alpha),abs((1-sa)*da*d[i]/alpha)))) for i in range(3)]
    else:scales=[1.]*3
    for i,scale in enumerate(scales):
        np.testing.assert_allclose(actual[i]/scale,target[i]/scale,rtol=4e-6,atol=max(8e-15,3e-6/scale),err_msg=str(case))
    np.testing.assert_allclose(actual[3],target[3],rtol=4e-6,atol=float(np.nextafter(np.float32(0),np.float32(1)))*2)
rendered=json.loads((OUT/'rendered.json').read_text(encoding='utf-8'));pixels=0
for case in rendered:
    actual=np.asarray(case['pixels'],dtype='f4').reshape(3,5,4)
    for y in range(3):
        for x in range(5):
            s=[-LIMIT if x%2==0 else LIMIT,LIMIT if y%2==0 else -LIMIT,D('-.5'),number(.4)];d=[-s[0],-s[1],D('2.125'),ONE];target=np.asarray(list(map(stored,over(case['mode'],s,d,ONE))))
            np.testing.assert_allclose(actual[y,x],target,rtol=4e-6,atol=3e-6)
    pixels+=15
print(f"25 blend modes: {len(m['color_cases'])} independent 110-digit Decimal colors, {len(m['cases'])} alpha/opacity combinations and {pixels} composed layer pixels; extended RGB/finite storage/extreme/tiny alpha checked")
