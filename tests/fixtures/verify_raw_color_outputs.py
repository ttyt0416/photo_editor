"""Independent single-illuminant Bradford, pseudo inverse and sensor decoding."""
from pathlib import Path
import hashlib,json,zipfile
import numpy as np
from verify_raw_outputs import parse,orient,SRGB
from verify_raw_temperature_outputs import white
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw-color';OUT=ROOT/'qa-output/raw-color'
manifest=json.loads((FIX/'manifest.json').read_text());inputs={case['file']:case for case in manifest};cases=json.loads((OUT/'cases.json').read_text())+json.loads((OUT/'pick-cases.json').read_text());pixels=0
B=np.array([[.8951,.2664,-.1614],[-.7502,1.7135,.0367],[.0389,-.0685,1.0296]]);Bi=np.linalg.inv(B)
whites={17:[1.0985,1,.35585],18:[.99072,1,.85223],19:[.98074,1,1.18232],23:[.96422,1,.82521],20:[.95682,1,.92149],21:[.95047,1,1.08883],22:[.94972,1,1.22638]};whites[1]=whites[21];whites[4]=whites[20]
for case in cases:
    item=inputs[case['input']];_,tags,sensor=parse(FIX/case['input']);channels=item['channels'];settings=case['settings']
    native=np.asarray(tags[50721],dtype='f4').reshape(channels,3).astype('f8')
    mode=settings['white_balance'];target_white=white(settings['temperature'],settings['tint']) if mode=='Temperature' else whites[item['illuminant']]
    adaptation=Bi@np.diag((B@target_white)/(B@whites[21]))@B
    if item['illuminant']==21 and mode!='Temperature':adaptation=np.eye(3)
    rgb2cam=native@adaptation@SRGB;rgb2cam/=rgb2cam.sum(axis=1,keepdims=True);camera2rgb=np.linalg.pinv(rgb2cam)
    mode=settings['white_balance']
    if mode=='Camera':gains=1/np.asarray(tags[50728],dtype='f8')
    elif mode=='Neutral':gains=np.ones(channels)
    elif mode=='Custom':gains=np.array(settings['gains']+([settings['gains_fourth']] if channels==4 else []),dtype='f8')
    else:
        neutral=native@white(settings['temperature'],settings['tint']);gains=neutral[1]/neutral
    if item['cfa']:
        assert list(tags[50710])==item['planes'];sensor=np.tile([sensor[0,0,0],sensor[0,1,0],sensor[1,0,0],sensor[1,1,0]],(10,12,1))
    if 'focus' in case:
        sampled=sensor
        if 50720 in tags:
            left,top=tags[50719];top+=tags[50829][0];left+=tags[50829][1];w,h=tags[50720];sampled=sampled[top:top+h,left:left+w]
        sampled=orient(sampled,tags[274][0]);h,w,_=sampled.shape;focus=np.array(case['focus'],dtype='f4');x,y=np.floor(focus*np.array([w-1,h-1],dtype='f4')+np.float32(.5)).astype(int);radius=case['radius'];average=sampled[max(0,y-radius):min(h,y+radius+1),max(0,x-radius):min(w,x+radius+1)].astype('f8').mean(axis=(0,1))
        np.testing.assert_allclose(gains,average[1]/average,rtol=3e-5,atol=3e-6)
        with zipfile.ZipFile(OUT/case['project']) as z:
            assert z.read('layers/1-source.raw')==(FIX/case['input']).read_bytes();m=json.loads(z.read('manifest.json'));assert m['version']==11;saved=m['layers'][0]['raw_source']['settings'];assert set(saved)==set(settings);assert saved['white_balance']==settings['white_balance'];assert saved['exposure']==settings['exposure'];np.testing.assert_array_equal(np.array(saved['gains'],dtype='f4'),np.array(settings['gains'],dtype='f4'));assert np.float32(saved['gains_fourth'])==np.float32(settings['gains_fourth'])
    rgb=(sensor.astype('f8')*gains)@camera2rgb.T*2**settings['exposure']
    gamma=np.where(rgb<=.0031308,12.92*rgb,1.055*np.maximum(rgb,0)**(1/2.4)-.055)
    if 50720 in tags:
        left,top=tags[50719];top+=tags[50829][0];left+=tags[50829][1];w,h=tags[50720];gamma=gamma[top:top+h,left:left+w]
    gamma=orient(gamma,tags[274][0]);target=np.concatenate([gamma,np.ones((*gamma.shape[:2],1))],axis=2).astype('f4');actual=np.fromfile(OUT/case['output'],dtype='<f4').reshape(target.shape)
    if item['cfa']:np.testing.assert_allclose(actual[2:-2,2:-2],target[2:-2,2:-2],rtol=4e-5,atol=6e-6)
    else:np.testing.assert_allclose(actual,target,rtol=4e-5,atol=6e-6,err_msg=str(case))
    assert np.isfinite(actual).all() and (actual[:,:,3]==1).all();pixels+=target.shape[0]*target.shape[1]
for item in manifest:
    source=(FIX/item['file']).read_bytes();assert hashlib.sha256(source).hexdigest()==item['source_sha256']
    with zipfile.ZipFile(OUT/(item['file']+'.rphoto')) as z:
        assert z.read('layers/1-source.raw')==source;m=json.loads(z.read('manifest.json'));assert m['version']==(11 if item['channels']==4 else 9)
        info=m['layers'][0]['raw_source']['info']
        if item['channels']==4:assert info['camera_wb_fourth']==item['wb'][3]
        if item['cfa']:assert info['camera_colors']==item['planes']
print(f'RAW color: {len(manifest)} independent 3/4-channel DNG sources, {len(cases)} Bradford/pseudo-inverse/WB/EV results/{pixels} pixels, 9 illuminants, SHORT/RATIONAL WB, linear/4-color CFA interior, CMYG/reordered planes, crop/orientation and v9/v11 native bytes')
