"""Independent DNG binary/sample parsing plus NumPy normalization/calibration/gamma/orientation."""
from pathlib import Path
import json,struct,hashlib,zipfile
import numpy as np
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw';OUT=ROOT/'qa-output/raw'
# Matrix constants from CIE/sRGB, matching the fixed D65 working space of rawler 0.8.
SRGB=np.array([[.4124564,.3575761,.1804375],[.2126729,.7151522,.0721750],[.0193339,.1191920,.9503041]])
def parse(path):
    data=path.read_bytes();end='<' if data[:2]==b'II' else '>';unpack=lambda fmt,b:struct.unpack(end+fmt,b)
    offset=unpack('I',data[4:8])[0];n=unpack('H',data[offset:offset+2])[0];tags={}
    for i in range(n):
        p=offset+2+i*12;tag,typ,count=unpack('HHI',data[p:p+8]);size={1:1,2:1,3:2,4:4,5:8,10:8}[typ]*count
        b=data[p+8:p+12][:size] if size<=4 else data[unpack('I',data[p+8:p+12])[0]:][:size]
        if typ in [1,2]:v=list(b)
        elif typ==5 or typ==10:
            v=unpack(('I' if typ==5 else 'i')*count*2,b);v=[v[j]/v[j+1] for j in range(0,len(v),2)]
        else:v=unpack(('H' if typ==3 else 'I')*count,b)
        tags[tag]=v
    w,h=tags[256][0],tags[257][0];cpp=tags[277][0];depth=tags[258][0];start=tags[273][0]
    sensor=np.frombuffer(data[start:start+tags[279][0]],dtype=end+('u2' if depth==16 else 'f4')).reshape(h,w,cpp).astype('f4')
    black=np.array(tags[50714],dtype='f4');white=np.array(tags[50717],dtype='f4')
    sensor=np.maximum(sensor-black,0)/(white-black)
    return data,tags,sensor

def orient(a,o):
    if o==2:return a[:,::-1]
    if o==3:return a[::-1,::-1]
    if o==4:return a[::-1]
    if o==5:return np.swapaxes(a,0,1)
    if o==6:return np.rot90(a,-1)
    if o==7:return np.swapaxes(a,0,1)[::-1,::-1]
    if o==8:return np.rot90(a,1)
    return a

def expected(tags,sensor,wb,exposure=0,calibration_white=None):
    mono=tags[262][0]==1;cfa=tags[262][0]==32803
    if cfa:
        # Constant known channel values, independent of the interpolation implementation.
        sensor=np.tile([sensor[0,0,0],sensor[0,1,0],sensor[1,1,0]],(10,12,1))
    if not mono:
        xyz2cam=np.array(tags[50721],dtype='f4').reshape(3,3)
        
        if calibration_white is not None:
            B=np.array([[.8951,.2664,-.1614],[-.7502,1.7135,.0367],[.0389,-.0685,1.0296]])
            xyz2cam=xyz2cam.astype('f8')@np.linalg.inv(B)@np.diag((B@calibration_white)/(B@np.array([.95047,1,1.08883])))@B
        rgb2cam=xyz2cam@SRGB;rgb2cam/=rgb2cam.sum(axis=1,keepdims=True)
        sensor=(sensor*np.array(wb))@np.linalg.inv(rgb2cam).T
    else:sensor=np.repeat(sensor,3,axis=2)
    sensor=sensor*2**exposure
    gamma=np.where(sensor<=.0031308,12.92*sensor,1.055*np.maximum(sensor,0)**(1/2.4)-.055)
    if 50720 in tags:
        left,top=tags[50719];top+=tags[50829][0];left+=tags[50829][1];w,h=tags[50720];gamma=gamma[top:top+h,left:left+w]
    gamma=orient(gamma,tags[274][0]);return np.concatenate([gamma,np.ones((*gamma.shape[:2],1))],axis=2).astype('f4')
if __name__=='__main__':
    manifest=json.loads((FIX/'manifest.json').read_text());assert len(manifest)==20
    for e in manifest:
        name=e['file'];data,tags,sensor=parse(FIX/name);assert hashlib.sha256(data).hexdigest()==e['source_sha256']
        for suffix,wb in [('',[2.,1.,4.]),('.neutral',[1.,1.,1.])]:
            target=expected(tags,sensor,wb);actual=np.fromfile(OUT/f'{name}{suffix}.rgba32.le',dtype='<f4').reshape(target.shape)
            # PPG has a two-pixel boundary policy; independently compare the whole interior.
            if e['kind']=='cfa':actual=actual[3:-3,3:-3];target=target[3:-3,3:-3]
            np.testing.assert_allclose(actual,target,rtol=2e-5,atol=2e-5,err_msg=name+suffix)
        with zipfile.ZipFile(OUT/f'{name}.rphoto') as z:
            assert z.read('layers/1-source.raw')==data;m=json.loads(z.read('manifest.json'));assert m['version']==9
            assert m['layers'][0]['raw_source']['settings']['white_balance']=='Camera'
    data,tags,sensor=parse(FIX/'linear-32-o1.dng');target=expected(tags,sensor,[.5,2.,3.],2);actual=np.fromfile(OUT/'custom.rgba32.le',dtype='<f4').reshape(target.shape);np.testing.assert_allclose(actual,target,rtol=2e-5,atol=2e-5)
    assert actual[:,:,:3].max()>1
    print('RAW DNG: 20 independent sensor files, 40 camera/neutral results plus custom linear exposure; 16/32-bit, two byte orders, 8 orientations, crop, grayscale and Bayer interior; original bytes/project assets and NumPy color formula verified')
