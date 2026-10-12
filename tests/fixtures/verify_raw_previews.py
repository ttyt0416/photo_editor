"""Independent NumPy alpha-weighted area thumbnail and exact f32 display crop."""
from pathlib import Path
import json
import numpy as np
from PIL import Image
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/raw-preview'
def display(p):
    return np.floor(np.clip(p,0,1).astype('f4')*np.float32(255)+np.float32(.5)).astype('u1')
def thumbnail(p):
    h,w=p.shape[:2];scale=min(512/w,256/h,1);tw=max(int(w*scale),1);th=max(int(h*scale),1)
    if (tw,th)==(w,h):return display(p)
    result=np.zeros((th,tw,4),dtype='u1');p=np.clip(p.astype('f8'),0,1);sx=w/tw;sy=h/th
    for y,x in np.ndindex(th,tw):
        left=x*sx;right=(x+1)*sx;top=y*sy;bottom=(y+1)*sy
        xs=np.arange(int(np.floor(left)),min(int(np.ceil(right)),w));ys=np.arange(int(np.floor(top)),min(int(np.ceil(bottom)),h))
        wx=np.minimum(right,xs+1)-np.maximum(left,xs);wy=np.minimum(bottom,ys+1)-np.maximum(top,ys);weight=wy[:,None]*wx[None,:];part=p[ys[:,None],xs[None,:]];a=part[:,:,3]*weight;alpha=a.sum()
        if alpha>0:result[y,x,:3]=np.floor((part[:,:,:3]*a[:,:,None]).sum(axis=(0,1))/alpha*255+.5).astype('u1')
        result[y,x,3]=np.floor(alpha/weight.sum()*255+.5).astype('u1')
    return result
manifest=json.loads((OUT/'manifest.json').read_text());max_error=0;count=0
for case in manifest:
    name=case['name'];w,h=case['size'];source=np.fromfile(OUT/f'{name}.rgba32.le',dtype='<f4').reshape(h,w,4);target=thumbnail(source);actual=np.asarray(Image.open(OUT/f'{name}-fit.png'));assert target.shape==actual.shape;error=np.max(np.abs(actual.astype('i2')-target.astype('i2')));assert error<=1;(max_error,count)=(max(max_error,int(error)),count+1)
    for i,focus in enumerate(case['focuses']):
        cw,ch=min(w,160),min(h,160);center=np.floor(np.asarray(focus,dtype='f4')*np.asarray([w-1,h-1],dtype='f4')+np.float32(.5)).astype('i8');x=np.clip(center[0]-cw//2,0,w-cw);y=np.clip(center[1]-ch//2,0,h-ch);target=display(source[y:y+ch,x:x+cw]);np.testing.assert_array_equal(np.asarray(Image.open(OUT/f'{name}-detail-{i}.png')),target);count+=1
print(f'RAW previews: {count} independent thumbnails/crops; max thumbnail rounding error {max_error}/255, exact display crops/alpha/HDR clipping and source coordinates')
