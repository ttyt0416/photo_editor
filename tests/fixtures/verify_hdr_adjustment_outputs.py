"""Independent float64 equations for point filters and alpha-weighted selection mixing."""
from pathlib import Path
import json, math, zipfile
import numpy as np
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/hdr-adjustments';m=json.loads((OUT/'formula.json').read_text(encoding='utf-8'));source=np.asarray(m['source'],dtype='f4');w,h=m['size'];LIMIT=float(np.finfo('f4').max)
def f(value):return float(np.float32(value))
def bounded(value):return np.clip(value,-LIMIT,LIMIT).astype('f4')
def curve(points,values):
    points=np.asarray(points,dtype='f4').astype('f8');r=np.searchsorted(points[:,0],values,side='left').clip(1,len(points)-1);a=points[r-1];b=points[r];return a[:,1]+(values-a[:,0])*(b[:,1]-a[:,1])/(b[:,0]-a[:,0])
def noise(x,y,c):
    seed=((x*0x9e3779b9)&0xffffffff)^((y*0x85ebca6b)&0xffffffff)^((c*0xc2b2ae35)&0xffffffff)
    seed^=seed>>16;seed=(seed*0x7feb352d)&0xffffffff;seed^=seed>>15;seed=(seed*0x846ca68b)&0xffffffff;seed^=seed>>16
    return f(np.float32(seed)/np.float32(0xffffffff)-np.float32(.5))
for index,case in enumerate(m['cases'][:10]):
    a=case['settings'];c=source[:,:3].astype('f8');alpha=source[:,3].copy()
    if a['black']!=0 or a['white']!=1 or a['gamma']!=1:
        with np.errstate(over='ignore'):c=bounded(np.maximum((c-f(a['black']))/(f(a['white'])-f(a['black'])),0)**(1/f(a['gamma']))).astype('f8')
    if a['exposure'] or a['contrast'] or a['brightness']:c=(c*2**f(a['exposure'])-.5)*(1+f(a['contrast']))+.5+f(a['brightness'])
    c[:,0]+=f(a['temperature'])*.15;c[:,2]-=f(a['temperature'])*.15;c[:,1]+=f(a['tint'])*.15
    shifts=np.asarray(a['color_balance']['shifts'],dtype='f8')
    if np.any(shifts):
        c=bounded(c).astype('f8');Y=c@np.array([.299,.587,.114]);position=Y.clip(0,1);shadow=(1-2*position).clip(0);highlight=(2*position-1).clip(0);weights=np.stack([shadow,1-shadow-highlight,highlight],axis=-1);changed=c+weights@shifts/200
        if a['color_balance']['preserve_luminosity']:
            changed-=((changed-c)@np.array([.299,.587,.114]))[:,None]
            for i in range(len(c)):
                if np.all((c[i]>=0)&(c[i]<=1)):
                    delta=changed[i]-Y[i];ratios=[1.]+[(1-Y[i])/v if v>0 else -Y[i]/v for v in delta if v!=0];changed[i]=Y[i]+delta*min(ratios)
        c=bounded(changed).astype('f8')
    Y=c@np.array([.299,.587,.114])
    if a['hue']:
        angle=np.float32(np.float32(a['hue'])*np.float32(np.pi/180));sn=float(np.sin(angle));cs=float(np.cos(angle));I=c@np.array([.596,-.274,-.322]);Q=c@np.array([.211,-.523,.312]);II=I*cs-Q*sn;QQ=I*sn+Q*cs;c=np.stack([Y+.956*II+.621*QQ,Y-.272*II-.647*QQ,Y-1.106*II+1.703*QQ],axis=-1)
    if a['monochrome']:c=np.repeat(Y[:,None],3,axis=1)
    elif a['saturation']:c=Y[:,None]+(c-Y[:,None])*(1+f(a['saturation']))
    if a['invert']:c=1-c
    for channel in range(3):c[:,channel]=curve(a['curves'][channel+1]['points'],curve(a['curves'][0]['points'],c[:,channel]))
    if a['gradient_map']:
        t=(c@np.array([.299,.587,.114])).clip(0,1);colors=np.asarray(a['gradient_colors'],dtype='f8');c=((1-t[:,None])*colors[0,:3]+t[:,None]*colors[1,:3])/255;alpha=(alpha.astype('f8')*((1-t)*colors[0,3]+t*colors[1,3])/255).astype('f4')
    for x in range(w):
        distance=((2*(x+.5)/w-1)**2+(2*.5/h-1)**2)/2
        c[x]*=1-f(a['vignette'])*distance
        for channel in range(3):c[x,channel]+=noise(x,0,0)*f(a['grain'])+noise(x,0,channel+1)*f(a['noise'])
    expected=np.column_stack([bounded(c),alpha]);actual=np.asarray(case['pixels'],dtype='f4').reshape(-1,4);np.testing.assert_allclose(actual,expected,rtol=4e-6,atol=2e-6,err_msg=f'point case {index}')
for case in m['scopes']:
    original=np.asarray(case['original'],dtype='f4').astype('f8');adjusted=np.asarray(case['adjusted'],dtype='f4').astype('f8');coverage=f(case['coverage'])
    if coverage<=0:expected=original.astype('f4')
    elif coverage>=1:expected=adjusted.astype('f4')
    else:
        alpha=original[3]*(1-coverage)+adjusted[3]*coverage;rgb=(original[:3]*original[3]*(1-coverage)+adjusted[:3]*adjusted[3]*coverage)/alpha if alpha>0 else original[:3]*(1-coverage)+adjusted[:3]*coverage;expected=np.append(bounded(rgb),np.float32(alpha))
    np.testing.assert_allclose(np.asarray(case['pixels'],dtype='f4'),expected,rtol=3e-6,atol=2e-6)
with zipfile.ZipFile(OUT/'extreme.rphoto') as z:assert json.loads(z.read('manifest.json'))['layers'][0]['adjustments']['hue']==75
print(f"HDR point filters: {len(m['cases'][:10])*len(source)} independently calculated pixels and {len(m['scopes'])} independent alpha-weighted mixes; finite extremes/negative/HDR/tiny alpha and saved settings verified; mixer/grading finite cases also covered by Rust regression tests")
