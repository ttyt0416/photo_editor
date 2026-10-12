"""Independent camera-neutral scalar roots and all-channel neutral sensor solve."""
from pathlib import Path
import json
import numpy as np
from verify_raw_outputs import parse,orient
from raw_profile_reference import model
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw-dual';OUT=ROOT/'qa-output/raw-dual'
inputs={x['file']:x for x in json.loads((FIX/'manifest.json').read_text())};custom=json.loads((OUT/'custom-cases.json').read_text());picks=json.loads((OUT/'pick-cases.json').read_text());pixels=0
for case in custom+picks:
    item=inputs[case['input']];_,tags,sensor=parse(FIX/case['input']);channels=item['channels'];settings=case['settings'];camera=1/np.asarray(tags[50728],dtype='f8')
    gains=np.array(settings['gains']+([settings.get('gains_fourth',camera[3])] if channels==4 else []),dtype='f4').astype('f8')
    if item['cfa']:sensor=np.tile([sensor[0,0,0],sensor[0,1,0],sensor[1,0,0],sensor[1,1,0]],(10,12,1))
    if 50720 in tags:
        left,top=tags[50719];top+=tags[50829][0];left+=tags[50829][1];w,h=tags[50720];sensor=sensor[top:top+h,left:left+w]
    sensor=orient(sensor,tags[274][0]);h,w,_=sensor.shape
    if 'focus' in case:
        focus=np.array(case['focus'],dtype='f4');x,y=np.floor(focus*np.array([w-1,h-1],dtype='f4')+np.float32(.5)).astype(int);radius=case['radius']
        average=sensor[max(0,y-radius):min(h,y+radius+1),max(0,x-radius):min(w,x+radius+1)].astype('f8').mean(axis=(0,1))
        reference=average[1]/average
        if channels==4:assert 'gains_fourth' in settings
        np.testing.assert_allclose(gains,reference,rtol=3e-5,atol=3e-6,err_msg=str(case))
    rgb=(sensor.astype('f8')*gains)@model(tags,channels,gains).T*2**settings['exposure'];gamma=np.where(rgb<=.0031308,12.92*rgb,1.055*np.maximum(rgb,0)**(1/2.4)-.055)
    target=np.concatenate([gamma,np.ones((*gamma.shape[:2],1))],axis=2).astype('f4');actual=np.fromfile(OUT/case['output'],dtype='<f4').reshape(target.shape)
    if item['cfa']:np.testing.assert_allclose(actual[2:-2,2:-2],target[2:-2,2:-2],rtol=4e-5,atol=6e-6)
    else:np.testing.assert_allclose(actual,target,rtol=4e-5,atol=6e-6,err_msg=str(case))
    assert np.isfinite(actual).all() and (actual[:,:,3]==1).all();pixels+=w*h
print(f'RAW custom dual: {len(custom)} gain results and {len(picks)} independently solved sensor patches/{pixels} pixels; SVD/Brent CCT, 3/4 channels, fourth override/fallback, CMYG interiors, crop/orientation')
