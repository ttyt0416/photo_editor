"""Independent hardware gain tags, spectral WB/Bradford and scalar-root profiles."""
from pathlib import Path
import json,hashlib,zipfile
import numpy as np
from verify_raw_outputs import parse,orient,SRGB
from verify_raw_temperature_outputs import white
from raw_profile_reference import model,cct,B,D65,whites
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw-analog';OUT=ROOT/'qa-output/raw-analog'
inputs={x['file']:x for x in json.loads((FIX/'manifest.json').read_text())};cases=json.loads((OUT/'cases.json').read_text());pixels=0
for case in cases:
    item=inputs[case['input']];data,tags,sensor=parse(FIX/case['input']);channels=item['channels'];settings=case['settings'];mode=settings['white_balance'];hardware=np.array(tags[50727],dtype='f4').astype('f8')
    original,t0,s0=parse(ROOT/'tests/fixtures'/item['source']);np.testing.assert_allclose(tags[50728],np.asarray(t0[50728])*hardware,rtol=1e-7,atol=1e-7)
    end='<' if original[:2]==b'II' else '>';depth=t0[258][0];cpp=t0[277][0];h0=t0[257][0];w0=t0[256][0];dtype=end+('u2' if depth==16 else 'f4')
    raw0=np.frombuffer(original[t0[273][0]:][:t0[279][0]],dtype=dtype).reshape(h0,w0,cpp).astype('f8');gain=np.asarray(tags[50727],dtype='f8')
    if item['cfa']:gain=np.array([[gain[(y%2)*2+x%2] for x in range(w0)] for y in range(h0)])[:,:,None]
    black=64 if depth==16 else 0;stored=(raw0-black)*gain+black
    if depth==16:stored=np.rint(stored)
    actual_stored=np.frombuffer(data[tags[273][0]:][:tags[279][0]],dtype=dtype).reshape(h0,w0,cpp);np.testing.assert_array_equal(actual_stored,stored.astype(dtype))
    for tag in [50721,50722]:
        if tag in tags:tags[tag]=(np.array(tags[tag],dtype='f4').reshape(channels,3).astype('f8')*hardware[:,None]).astype('f4').ravel().tolist()
    camera=(1/np.array(tags[50728],dtype='f8')).astype('f4').astype('f8')
    if mode=='Camera':gains=camera
    elif mode=='Neutral':gains=np.ones(channels)
    elif mode=='Custom':gains=np.array(settings['gains']+([settings.get('gains_fourth',camera[3])] if channels==4 else []),dtype='f4').astype('f8')
    else:
        k=settings.get('temperature',6500);point=white(k,settings.get('tint',0));native=np.array(tags[50721],dtype='f4').reshape(channels,3).astype('f8')
        if item['dual']:
            a,b=[cct(whites[tags[i][0]]) for i in [50778,50779]];weight=np.clip((1/k-1/b)/(1/a-1/b),0,1);m2=np.array(tags[50722],dtype='f4').reshape(channels,3).astype('f8');native=(weight*native+(1-weight)*m2).astype('f4').astype('f8')
        neutral=native@point;gains=(neutral[1]/neutral).astype('f4').astype('f8')
    if item['dual'] and mode in ['Camera','Custom']:camera2rgb=model(tags,channels,gains)
    else:
        lights=[tags[50778][0]]+([tags[50779][0]] if item['dual'] else []);selected=next(light for light in [21,17,23,22] if light in lights);slot=lights.index(selected)
        native=np.array(tags[50721+slot],dtype='f4').reshape(channels,3).astype('f8')
        if mode=='Temperature':
            k=settings.get('temperature',6500);point=white(k,settings.get('tint',0))
            if item['dual']:
                a,b=[cct(whites[i]) for i in lights];weight=np.clip((1/k-1/b)/(1/a-1/b),0,1);native=(weight*np.array(tags[50721]).reshape(channels,3)+(1-weight)*np.array(tags[50722]).reshape(channels,3)).astype('f4').astype('f8')
        else:point=whites[selected]
        calibrated=(native@np.linalg.inv(B)@np.diag((B@point)/(B@D65))@B).astype('f4').astype('f8')
        rgb2cam=calibrated@SRGB;rgb2cam/=rgb2cam.sum(axis=1,keepdims=True);camera2rgb=np.linalg.pinv(rgb2cam).astype('f4').astype('f8')
    if item['cfa']:sensor=np.tile([sensor[0,0,0],sensor[0,1,0],sensor[1,0,0],sensor[1,1,0]],(10,12,1))
    if 50720 in tags:
        left,top=tags[50719];top+=tags[50829][0];left+=tags[50829][1];w,h=tags[50720];sensor=sensor[top:top+h,left:left+w]
    sensor=orient(sensor,tags[274][0]);h,w,_=sensor.shape
    if 'focus' in case:
        focus=np.array(case['focus'],dtype='f4');x,y=np.floor(focus*np.array([w-1,h-1],dtype='f4')+np.float32(.5)).astype(int);r=case['radius'];avg=sensor[max(0,y-r):min(h,y+r+1),max(0,x-r):min(w,x+r+1)].astype('f8').mean(axis=(0,1));np.testing.assert_allclose(gains,avg[1]/avg,rtol=3e-5,atol=3e-6)
    rgb=(sensor.astype('f8')*gains)@camera2rgb.T*2**settings['exposure'];gamma=np.where(rgb<=.0031308,12.92*rgb,1.055*np.maximum(rgb,0)**(1/2.4)-.055);target=np.concatenate([gamma,np.ones((*gamma.shape[:2],1))],axis=2).astype('f4');actual=np.fromfile(OUT/case['output'],dtype='<f4').reshape(target.shape)
    if item['cfa']:np.testing.assert_allclose(actual[2:-2,2:-2],target[2:-2,2:-2],rtol=4e-5,atol=6e-6)
    else:np.testing.assert_allclose(actual,target,rtol=4e-5,atol=6e-6,err_msg=str(case))
    assert np.isfinite(actual).all() and (actual[:,:,3]==1).all();pixels+=w*h
for item in inputs.values():
    data=(FIX/item['file']).read_bytes();assert hashlib.sha256(data).hexdigest()==item['source_sha256']
    with zipfile.ZipFile(OUT/(item['file']+'.rphoto')) as z:
        assert z.read('layers/1-source.raw')==data;m=json.loads(z.read('manifest.json'));assert m['version']==(11 if item['channels']==4 else 9)
print(f'RAW AnalogBalance: {len(inputs)} own non-unity/identity inputs, {len(cases)} independently decoded/modelled results/{pixels} pixels; hardware neutral tags, 3/4 channels, single/dual profiles, WB modes/picker, CMYG interior, crop/orientation and native source preservation')
