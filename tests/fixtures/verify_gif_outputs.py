"""Independently decode GIF palettes, transparency, interlacing and dithering."""
from pathlib import Path
from PIL import Image
import numpy as np
root=Path(__file__).resolve().parents[2]/'qa-output/gif-output'
source=np.asarray(Image.open(root/'source.png').convert('RGBA')).astype(np.uint16)
def descriptor(data):
    assert data[:6] in (b'GIF87a',b'GIF89a') and data[-1:]==b';'
    i=13+(3*(2**((data[10]&7)+1)) if data[10]&128 else 0)
    transparency=None
    while i<len(data):
        block=data[i];i+=1
        if block==33:
            label=data[i];i+=1
            if label==249:
                assert data[i]==4
                transparency=data[i+4] if data[i+1]&1 else None
            while data[i]:i+=1+data[i]
            i+=1
        elif block==44:
            flags=data[i+8]
            return bool(flags&64),transparency,2**((flags&7)+1)
        else:raise AssertionError(f'unexpected block {block}')
count=0
for colors in (2,16,256):
 for dither in (0,1):
  for transparent in (0,1):
   pair=[]
   for interlace in (0,1):
    path=root/f'c{colors}-d{dither}-t{transparent}-i{interlace}.gif'
    flags,index,size=descriptor(path.read_bytes())
    assert flags==bool(interlace) and (index is not None)==bool(transparent)
    assert size<=colors
    im=Image.open(path);assert im.size==(47,33) and im.n_frames==1
    actual=np.asarray(im.convert('RGBA'))
    alpha=np.where(source[:,:,3]<128,0,255) if transparent else np.full((33,47),255)
    assert np.array_equal(actual[:,:,3],alpha)
    bg=np.array([20,30,40],dtype=np.uint16)
    expected=(source[:,:,:3]*source[:,:,3,None]+bg*(255-source[:,:,3,None])+127)//255
    if colors==256:
        assert np.mean(abs(actual[:,:,:3].astype(float)[alpha>0]-expected.astype(float)[alpha>0]))<10
    indices=np.asarray(im)
    assert len(np.unique(indices))<=colors
    if index is not None:assert not np.any((indices==index)&(alpha>0))
    pair.append(actual)
    count+=1
   assert np.array_equal(*pair), 'interlaced pixel row order changed'
assert count==24
for threshold in (1,127,128,255):
 actual=np.asarray(Image.open(root/f'threshold{threshold}.gif').convert('RGBA'))
 assert np.array_equal(actual[:,:,3],np.where(source[:,:,3]<threshold,0,255))
exact=np.asarray(Image.open(root/'exact.gif').convert('RGBA'))
colors=np.array([[0,0,0,0],[0,0,0,255],[255,0,0,255],[0,255,0,255]],dtype=np.uint8)
assert np.array_equal(exact,colors[np.fromfunction(lambda y,x:(x+y)%4,(11,13),dtype=int)])
for interlace in (0,1):
 assert not np.any(np.asarray(Image.open(root/f'transparent{interlace}.gif').convert('RGBA'))[:,:,3])
ramp=np.asarray(Image.open(root/'ramp.png').convert('RGB')).astype(float)
errors=[]
for dither in (0,1):
 actual=np.asarray(Image.open(root/f'ramp{dither}.gif').convert('RGB')).astype(float)
 errors.append(np.mean(abs(actual[:,32:96,0].mean(axis=0)-ramp[0,32:96,0])))
assert errors[1]<errors[0]*.2
print(f'GIF: 24 palette/dither/alpha/interlace combinations, exact black+transparent palette, thresholds 1/127/128/255, row order and ramp improvement {errors} verified with Pillow')
