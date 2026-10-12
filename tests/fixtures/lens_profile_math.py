"""Independent Lensfun radial/tangential equations, float sampler and RAW scope verification."""
from pathlib import Path
import json, math, hashlib, zipfile, xml.etree.ElementTree as ET
import numpy as np
from psd_tools import PSDImage
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/lens-profiles';F=np.float32

def coordinates(p,size):
    w,h=size;ys,xs=np.indices((h,w));short=min(w-1,h-1)
    if not p["enabled"] or not any(p["coefficients"]):return np.stack([xs+.5,ys+.5],axis=-1)
    cx=w/2+short/2*p['center'][0];cy=h/2+short/2*p['center'][1]
    scale=math.hypot(36,24)/(p['input_crop']*math.hypot(w,h)*p['real_focal']) if p['model']=='acm' else 2*p['crop']*math.hypot(p['aspect'],1)/(p['input_crop']*math.hypot(w,h))
    x=(xs+.5-cx)*scale;y=(ys+.5-cy)*scale;q=x*x+y*y;r=np.sqrt(q);a,b,c,d,e=p['coefficients']
    if p['model']=='poly3':factor=1+a/(1-a)**3*q
    elif p['model']=='poly5':factor=1+a*q+b*q*q
    elif p['model']=='ptlens':
        z=1-a-b-c;factor=1+a/z**4*r*q+b/z**3*q+c/z**2*r
    else:factor=1+a*q+b*q*q+c*q*q*q
    dx=x*factor;dy=y*factor
    if p['model']=='acm':dx+=2*x*(d*y+e*x)+e*q;dy+=2*y*(d*y+e*x)+d*q
    return np.stack([cx+dx/scale,cy+dy/scale],axis=-1)

def cubic(x):
    x=abs(F(x))
    if x<=1:return F(F(F(F(1.5)*x)*x)*x)-F(F(F(2.5)*x)*x)+F(1)
    if x<2:return F(F(F(F(-.5)*x)*x)*x)+F(F(F(2.5)*x)*x)-F(F(4)*x)+F(2)
    return F(0)

def sample(source,points,mode):
    out=np.zeros_like(source);h,w=source.shape[:2]
    for y,x in np.ndindex((h,w)):
        pt=points[y,x].astype('f4')
        if mode=='Nearest':
            ix,iy=np.floor(pt).astype(int)
            if 0<=ix<w and 0<=iy<h:out[y,x]=source[iy,ix]
            continue
        pt=pt-F(.5);base=np.floor(pt).astype(int);t=pt-base.astype('f4');s=np.zeros(4,dtype='f8')
        for iy in (range(2) if mode=='Bilinear' else range(-1,3)):
            for ix in (range(2) if mode=='Bilinear' else range(-1,3)):
                weight=F((F(1)-t[0] if ix==0 else t[0])*(F(1)-t[1] if iy==0 else t[1])) if mode=='Bilinear' else F(cubic(t[0]-F(ix))*cubic(t[1]-F(iy)))
                nx=base[0]+ix;ny=base[1]+iy
                if 0<=nx<w and 0<=ny<h:
                    rgba=source[ny,nx].astype('f8');alpha=np.clip(rgba[3],0,1);s[:3]+=rgba[:3]*alpha*float(weight);s[3]+=alpha*float(weight)
        if s[3]>0:out[y,x,:3]=np.clip(s[:3]/s[3],-np.finfo('f4').max,np.finfo('f4').max).astype('f4');out[y,x,3]=F(np.clip(s[3],0,1))
    return out

