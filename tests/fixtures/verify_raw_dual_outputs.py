"""Independent spectral CCT minimization, inverse-CCT interpolation and Bradford/pinv."""
from pathlib import Path
import json,hashlib,zipfile
import numpy as np
from verify_raw_outputs import parse,orient,SRGB
from verify_raw_temperature_outputs import white,locus,uv
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw-dual';OUT=ROOT/'qa-output/raw-dual'
from raw_profile_reference import cct,B,D65,whites
for case in json.loads((OUT/'ccts.json').read_text()):np.testing.assert_allclose(case['temperature'],cct(case['white']),rtol=2e-7,atol=.01)
manifest=json.loads((FIX/'manifest.json').read_text());inputs={x['file']:x for x in manifest};cases=json.loads((OUT/'cases.json').read_text());pixels=0
for case in cases:
    item=inputs[case['input']];data,tags,sensor=parse(FIX/case['input']);channels=item['channels'];settings=case['settings'];k=settings.get('temperature',6500);t=settings.get('tint',0)
    a,b=[cct(whites[i]) for i in item['lights']];weight=np.clip((1/k-1/b)/(1/a-1/b),0,1)
    matrices=[np.asarray(tags[tag],dtype='f4').reshape(channels,3).astype('f8') for tag in [50721,50722]]
    native=(weight*matrices[0]+(1-weight)*matrices[1]).astype('f4').astype('f8');point=white(k,t)
    neutral=native@point;gains=(neutral[1]/neutral).astype('f4').astype('f8')
    calibrated=(native@np.linalg.inv(B)@np.diag((B@point)/(B@D65))@B).astype('f4').astype('f8')
    rgb2cam=calibrated@SRGB;rgb2cam/=rgb2cam.sum(axis=1,keepdims=True);camera2rgb=np.linalg.pinv(rgb2cam).astype('f4').astype('f8')
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
    with zipfile.ZipFile(OUT/(item['file']+'.rphoto')) as z:
        assert z.read('layers/1-source.raw')==data;m=json.loads(z.read('manifest.json'));assert m['version']==(11 if item['channels']==4 else 10)
        s=m['layers'][0]['raw_source']['settings'];assert s['white_balance']=='Temperature' and s['temperature']==4000 and s['tint']==75
print(f'RAW dual: {len(manifest)} own inputs, {len(cases)} independent interpolated/Bradford color results/{pixels} pixels; SciPy CCT, reversed slots, clamped endpoints, tint, 3/4 channels and CMYG CFA interiors, v10/v11 and exact native bytes')
