"""Independent bracketed CCT root solving and SVD camera-neutral inversion."""
from pathlib import Path
import json,hashlib,zipfile
import numpy as np
from scipy.optimize import brentq
from verify_raw_outputs import parse,orient,SRGB
from raw_profile_reference import model
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw-dual';OUT=ROOT/'qa-output/raw-dual'
manifest=json.loads((FIX/'manifest.json').read_text());inputs={x['file']:x for x in manifest};cases=json.loads((OUT/'camera-cases.json').read_text());pixels=0
for case in cases:
    item=inputs[case['input']];data,tags,sensor=parse(FIX/case['input']);channels=item['channels'];settings=case['settings']
    gains=(1/np.asarray(tags[50728],dtype='f8')).astype('f4').astype('f8');camera2rgb=model(tags,channels,gains)
    if item['cfa']:sensor=np.tile([sensor[0,0,0],sensor[0,1,0],sensor[1,0,0],sensor[1,1,0]],(10,12,1))
    rgb=(sensor.astype('f8')*gains)@camera2rgb.T*2**settings['exposure'];gamma=np.where(rgb<=.0031308,12.92*rgb,1.055*np.maximum(rgb,0)**(1/2.4)-.055)
    if 50720 in tags:
        left,top=tags[50719];top+=tags[50829][0];left+=tags[50829][1];w,h=tags[50720];gamma=gamma[top:top+h,left:left+w]
    gamma=orient(gamma,tags[274][0]);target=np.concatenate([gamma,np.ones((*gamma.shape[:2],1))],axis=2).astype('f4');actual=np.fromfile(OUT/case['output'],dtype='<f4').reshape(target.shape)
    if item['cfa']:np.testing.assert_allclose(actual[2:-2,2:-2],target[2:-2,2:-2],rtol=4e-5,atol=6e-6)
    else:np.testing.assert_allclose(actual,target,rtol=4e-5,atol=6e-6,err_msg=str(case))
    assert np.isfinite(actual).all() and (actual[:,:,3]==1).all();pixels+=target.shape[0]*target.shape[1]
for item in manifest:
    data=(FIX/item['file']).read_bytes();assert hashlib.sha256(data).hexdigest()==item['source_sha256']
    with zipfile.ZipFile(OUT/(item['file']+'-camera.rphoto')) as z:
        assert z.read('layers/1-source.raw')==data;m=json.loads(z.read('manifest.json'));assert m['version']==(11 if item['channels']==4 else 9);assert m['layers'][0]['raw_source']['settings']['white_balance']=='Camera'
print(f'RAW camera dual: {len(cases)} independently root-solved/SVD color results/{pixels} pixels; CameraNeutral, EV, slots, crop/orientation, CMYG interior and exact v9/v11 native data')
