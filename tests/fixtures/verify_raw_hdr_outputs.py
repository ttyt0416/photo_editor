"""Independent wide NumPy RAW color arithmetic and native source preservation."""
from pathlib import Path
import hashlib,json,zipfile
import numpy as np
from verify_raw_outputs import parse,orient,SRGB
from verify_raw_temperature_outputs import white
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw-hdr';OUT=ROOT/'qa-output/raw-hdr';maximum=float(np.finfo('f4').max)
manifest=json.loads((FIX/'manifest.json').read_text());cases=json.loads((OUT/'cases.json').read_text());pixels=0
for item in manifest:assert hashlib.sha256((FIX/item['file']).read_bytes()).hexdigest()==item['source_sha256']
for case in cases:
    _,tags,sensor=parse(FIX/case['input']);settings=case['settings'];mono=tags[262][0]==1
    rgb=sensor.astype('f8')
    if mono:rgb=np.repeat(rgb,3,axis=2)
    else:
        xyz2cam=np.asarray(tags[50721],dtype='f4').reshape(3,3)
        mode=settings['white_balance']
        model=xyz2cam.astype('f8')
        if mode=='Temperature':
            B=np.array([[.8951,.2664,-.1614],[-.7502,1.7135,.0367],[.0389,-.0685,1.0296]])
            model=model@np.linalg.inv(B)@np.diag((B@white(settings['temperature'],settings['tint']))/(B@np.array([.95047,1,1.08883])))@B
        rgb2cam=model@SRGB
        rgb2cam/=rgb2cam.sum(axis=1,keepdims=True)
        camera2rgb=np.linalg.inv(rgb2cam)
        mode=settings['white_balance']
        if mode=='Camera':gains=1/np.asarray(tags[50728],dtype='f8');gains/=gains[1]
        elif mode=='Neutral':gains=np.ones(3)
        elif mode=='Custom':gains=np.asarray(settings['gains'],dtype='f8')
        else:
            neutral=xyz2cam.astype('f8')@white(settings['temperature'],settings['tint']);gains=neutral[1]/neutral
        rgb=(rgb*gains)@camera2rgb.T
    rgb*=2**settings['exposure']
    encoded=np.where(rgb<=.0031308,12.92*rgb,1.055*np.maximum(rgb,0)**(1/2.4)-.055)
    encoded=np.clip(encoded,-maximum,maximum).astype('f4')
    if 50720 in tags:
        left,top=tags[50719];top+=tags[50829][0];left+=tags[50829][1];w,h=tags[50720];encoded=encoded[top:top+h,left:left+w]
    encoded=orient(encoded,tags[274][0]);target=np.concatenate([encoded,np.ones((*encoded.shape[:2],1),dtype='f4')],axis=2)
    actual=np.fromfile(OUT/case['output'],dtype='<f4').reshape(target.shape);assert np.isfinite(actual).all()
    np.testing.assert_allclose(actual,target,rtol=3e-5,atol=1e-36,err_msg=str(case));pixels+=target.shape[0]*target.shape[1]
with zipfile.ZipFile(OUT/'developed.rphoto') as z:
    assert z.read('layers/1-source.raw')==(FIX/'linear-32-o1-hdr.dng').read_bytes();m=json.loads(z.read('manifest.json'));assert m['version']==9
    assert m['layers'][0]['raw_source']['settings']['exposure']==16
print(f'RAW HDR: {len(cases)} independently parsed wide color results/{pixels} pixels, finite float32 maximum/tiny/zero sensors, signed/clamped storage, EV -16/0/+16, four WB modes, crop/orientation and byte-identical native RAW source')
