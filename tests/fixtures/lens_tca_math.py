"""Independent three TCA models, order of optics and per-channel unassociated float sampling."""
from lens_profile_math import *
OUT=ROOT/'qa-output/lens-tca'
def channel_points(p,base,blue):
    h,w=base.shape[:2];tca=p['tca'];v=tca['blue' if blue else 'red'];a,b,c,d,e,f=v;short=min(w-1,h-1);cx=w/2+short/2*p['center'][0];cy=h/2+short/2*p['center'][1]
    scale=math.hypot(36,24)/(p['input_crop']*math.hypot(w,h)*p['real_focal']) if tca['model']=='acm' else 2*p['crop']*math.hypot(p['aspect'],1)/(p['input_crop']*math.hypot(w,h))
    x=(base[:,:,0]-cx)*scale;y=(base[:,:,1]-cy)*scale;q=x*x+y*y
    if tca['model']=='linear':dx=a*x;dy=a*y
    elif tca['model']=='poly3':gain=a+b*np.sqrt(q)+c*q;dx=x*gain;dy=y*gain
    else:
        gain=1+b*q+c*q*q+d*q*q*q+2*(e*y+f*x);dx=a*(x*gain+f*q);dy=a*(y*gain+e*q)
    return np.stack([cx+dx/scale,cy+dy/scale],axis=-1)
def transform(source,p,sampling):
    h,w=source.shape[:2];base=coordinates(p,[w,h])
    if not p.get('tca') or not p['tca']['enabled']:
        target=sample(source,base,sampling) if p['enabled'] and any(p['coefficients']) else source.copy()
        return target,np.stack([base,base,base],axis=2)
    red=channel_points(p,base,False);blue=channel_points(p,base,True)
    target=sample(source,base,sampling) if p['enabled'] and any(p['coefficients']) else source.copy()
    target[:,:,0]=sample(source,red,sampling)[:,:,0];target[:,:,2]=sample(source,blue,sampling)[:,:,2]
    return target,np.stack([red,base,blue],axis=2)
