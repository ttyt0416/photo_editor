"""Independent PA/ACM polynomial and extended-sRGB linear light before optics."""
from lens_tca_math import *
OUT=ROOT/'qa-output/lens-vignetting'
def correct(source,p):
    v=p['vignetting'][p['vignetting_selected']];h,w=source.shape[:2];ys,xs=np.indices((h,w));short=min(w-1,h-1);cx=w/2+short/2*p['center'][0];cy=h/2+short/2*p['center'][1]
    scale=2*p['crop']/(p['input_crop']*math.hypot(w,h)) if v['model']=='pa' else math.hypot(36,24)/(p['input_crop']*math.hypot(w,h)*p['real_focal']);q=((xs+.5-cx)*scale)**2+((ys+.5-cy)*scale)**2;a,b,c=v['coefficients'];divisor=1+a*q+b*q*q+c*q*q*q
    target=source.copy();rgb=source[:,:,:3].astype('f8');linear=rgb/12.92;high=rgb>0.04045;linear[high]=((rgb[high]+.055)/1.055)**2.4;linear/=divisor[:,:,None];encoded=linear*12.92;high=linear>0.0031308;encoded[high]=1.055*linear[high]**(1/2.4)-.055;target[:,:,:3]=np.clip(encoded,-np.finfo('f4').max,np.finfo('f4').max).astype('f4');target[source[:,:,3]<=0]=source[source[:,:,3]<=0];return target
