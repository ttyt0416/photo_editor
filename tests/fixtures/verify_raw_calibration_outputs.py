"""Independent DNG CC/signature decoding, separate interpolation and SVD/CCT root."""
from pathlib import Path
import json,hashlib,zipfile
import numpy as np
from scipy.optimize import brentq
from verify_raw_outputs import parse,orient,SRGB
from verify_raw_temperature_outputs import white
from raw_profile_reference import cct,B,D65,whites
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw-calibration';OUT=ROOT/'qa-output/raw-calibration'
inputs={x['file']:x for x in json.loads((FIX/'manifest.json').read_text())};cases=json.loads((OUT/'cases.json').read_text());pixels=0;camera_values={};separate_differences=0

def profile(tags,channels):
    hardware=np.array(tags.get(50727,[1]*channels),dtype='f4').astype('f8')
    lights=[tags[50778][0]]+([tags[50779][0]] if 50722 in tags else [])
    temperatures=[cct(whites[light]) for light in lights]
    cm=[(np.array(tags[50721+i],dtype='f4').reshape(channels,3).astype('f8')*hardware[:,None]).astype('f4').astype('f8') for i in range(len(lights))]
    def signature(tag):
        value=bytes(tags.get(tag,[0]));assert value[-1]==0 and 0 not in value[:-1];return value[:-1].decode('utf8')
    matched=signature(50931)==signature(50932)
    cc=[np.array(tags.get(50723+i,np.eye(channels).ravel()),dtype='f8').reshape(channels,channels) if matched else np.eye(channels) for i in range(len(lights))]
    def apply(native,cal):
        return ((cal*(hardware[:,None]/hardware[None,:]))@native).astype('f4').astype('f8')
    def weight(k):return np.clip((1/k-1/temperatures[1])/(1/temperatures[0]-1/temperatures[1]),0,1)
    def native(k):
        if len(cm)==1:return apply(cm[0],cc[0])
        w=weight(k);base=(w*cm[0]+(1-w)*cm[1]).astype('f4').astype('f8');cal=w*cc[0]+(1-w)*cc[1];return apply(base,cal)
    def selected():
        i=next(lights.index(light) for light in [21,17,23,22] if light in lights)
        return apply(cm[i],cc[i]),np.array(whites[lights[i]])
    return native,selected,cm,cc,apply,weight

def transform(native,point):
    adapted=(native@np.linalg.inv(B)@np.diag((B@point)/(B@D65))@B).astype('f4').astype('f8')
    rgb2cam=adapted@SRGB;rgb2cam/=rgb2cam.sum(axis=1,keepdims=True)
    return np.linalg.pinv(rgb2cam).astype('f4').astype('f8')

def dual_model(native,gains):
    neutral=1/np.array(gains,dtype='f4').astype('f8')
    def point(k):
        xyz=np.linalg.pinv(native(k))@neutral;return xyz/xyz[1]
    root=brentq(lambda t:np.log(cct(point(np.exp(t))))-t,np.log(1000),np.log(50000),xtol=1e-7)
    k=np.exp(root);w=point(k);assert (w>0).all();return transform(native(k),w)

for case in cases:
    item=inputs[case['input']];data,tags,sensor=parse(FIX/case['input']);channels=item['channels'];settings=case['settings'];mode=settings['white_balance'];native,selected,cm,cc,apply,weight=profile(tags,channels)
    original,t0,_=parse(ROOT/'tests/fixtures'/item['source']);assert original[t0[273][0]:][:t0[279][0]]==data[tags[273][0]:][:tags[279][0]]
    if item['dual'] and item['variant']=='match':
        midpoint=2/(1/cct(whites[tags[50778][0]])+1/cct(whites[tags[50779][0]]));w=weight(midpoint);precombined=w*apply(cm[0],cc[0])+(1-w)*apply(cm[1],cc[1]);assert np.max(np.abs(precombined-native(midpoint)))>1e-5;separate_differences+=1
    if item['xy']:
        x,y=tags[50729];point=np.array([x/y,1,(1-x-y)/y]);neutral=native(cct(point))@point;camera=(neutral[1]/neutral).astype('f4').astype('f8')
    else:camera=(1/np.array(tags[50728],dtype='f8')).astype('f4').astype('f8')
    camera_values[item['file']]=camera
    if mode=='Camera':gains=camera
    elif mode=='Neutral':gains=np.ones(channels)
    elif mode=='Custom':gains=np.array(settings['gains']+([settings.get('gains_fourth',camera[3])] if channels==4 else []),dtype='f4').astype('f8')
    else:
        k=settings['temperature'];point=white(k,settings['tint']);neutral=native(k)@point;gains=(neutral[1]/neutral).astype('f4').astype('f8')
    if item['dual'] and mode in ['Camera','Custom']:camera2rgb=dual_model(native,gains)
    elif mode=='Temperature':camera2rgb=transform(native(settings['temperature']),white(settings['temperature'],settings['tint']))
    else:camera2rgb=transform(*selected())
    if item['cfa']:sensor=np.tile([sensor[0,0,0],sensor[0,1,0],sensor[1,0,0],sensor[1,1,0]],(10,12,1))
    if 50720 in tags:
        left,top=tags[50719];top+=tags[50829][0];left+=tags[50829][1];w,h=tags[50720];sensor=sensor[top:top+h,left:left+w]
    sensor=orient(sensor,tags[274][0]);h,w,_=sensor.shape
    if 'focus' in case:
        focus=np.array(case['focus'],dtype='f4');x,y=np.floor(focus*np.array([w-1,h-1],dtype='f4')+np.float32(.5)).astype(int);r=case['radius'];avg=sensor[max(0,y-r):min(h,y+r+1),max(0,x-r):min(w,x+r+1)].astype('f8').mean(axis=(0,1));np.testing.assert_allclose(gains,avg[1]/avg,rtol=3e-5,atol=3e-6)
    rgb=(sensor.astype('f8')*gains)@camera2rgb.T*2**settings['exposure'];gamma=np.where(rgb<=.0031308,12.92*rgb,1.055*np.maximum(rgb,0)**(1/2.4)-.055);target=np.concatenate([gamma,np.ones((*gamma.shape[:2],1))],axis=2).astype('f4');actual=np.fromfile(OUT/case['output'],dtype='<f4').reshape(target.shape)
    if item['cfa']:np.testing.assert_allclose(actual[2:-2,2:-2],target[2:-2,2:-2],rtol=4e-5,atol=6e-6,err_msg=str(case))
    else:np.testing.assert_allclose(actual,target,rtol=4e-5,atol=6e-6,err_msg=str(case))
    assert np.isfinite(actual).all() and (actual[:,:,3]==1).all();pixels+=w*h
assert separate_differences>0
for item in inputs.values():
    data=(FIX/item['file']).read_bytes();assert hashlib.sha256(data).hexdigest()==item['source_sha256']
    with zipfile.ZipFile(OUT/(item['file']+'.rphoto')) as z:
        assert z.read('layers/1-source.raw')==data;m=json.loads(z.read('manifest.json'));assert m['version']==(11 if item['channels']==4 else 9)
        info=m['layers'][0]['raw_source']['info'];np.testing.assert_allclose(info['camera_wb'],camera_values[item['file']][:3],rtol=5e-7,atol=1e-7)
        if item['channels']==4:np.testing.assert_allclose(info['camera_wb_fourth'],camera_values[item['file']][3],rtol=5e-7,atol=1e-7)
print(f'RAW CameraCalibration: {len(inputs)} own sources, {len(cases)} independently calculated results/{pixels} pixels; UTF8 signatures/default/mismatch/identity, independent CM/CC blending (distinct from blending combined endpoints), AB order, 3/4 sensors, xy/neutral, four WB modes/picker, SVD/CCT root, native bytes/project metadata')
