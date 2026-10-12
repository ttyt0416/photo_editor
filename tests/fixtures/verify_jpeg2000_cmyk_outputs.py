"""C raw component decoding + independently rational CMYK complement conversion."""
from pathlib import Path
import json,os,struct
os.environ['OPENCV_LOG_LEVEL']='SILENT'
import numpy as np,cv2
from jpeg2000_c_reference import components,version
OUT=Path(__file__).resolve().parents[2]/'qa-output/jpeg2000-input/cmyk'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==18
for case in cases:
    path=OUT/case['file'];planes=components(path).astype('i8');precisions=case['precisions'];signed=case['signed'];target=(1<<case['depth'])-1
    limit=np.array([(1<<p)-1 for p in precisions],dtype='i8')
    rgb=(limit[:3]-np.clip(planes[:,:,:3],0,limit[:3]))*(limit[3]-np.clip(planes[:,:,3],0,limit[3]))[:,:,None]*target//(limit[:3]*limit[3])
    alpha=(np.clip(planes[:,:,4]+((1<<(precisions[4]-1)) if signed[4] else 0),0,limit[4])*target//limit[4]) if len(precisions)==5 else np.full((5,7),target)
    rgba=np.concatenate([rgb,alpha[:,:,None]],2)
    actual=np.fromfile(OUT/case['decoded'],dtype='<f4').reshape(5,7,4)
    np.testing.assert_allclose(actual,rgba.astype('f4')/np.float32(target),rtol=0,atol=6e-8,err_msg=case['file'])
    encoded=path.read_bytes();at=encoded.index(b'colr');assert struct.unpack_from('>I',encoded,at+7)[0]==12
    if len(precisions)==5:
        at=encoded.index(b'cdef')+4;assert struct.unpack_from('>H',encoded,at)[0]==5
        assert struct.unpack_from('>HHH',encoded,at+2+4*6)==(4,1,0)
    rgba=rgba.copy();rgba[:,:,:3][alpha==0]=0
    png=cv2.imdecode(np.frombuffer((OUT/(case['file']+'.png')).read_bytes(),'u1'),cv2.IMREAD_UNCHANGED)
    np.testing.assert_array_equal(png[:,:,[2,1,0,3]],rgba.astype('u1' if case['depth']==8 else 'u2'))
print(f'CMYK JPEG2000: C OpenJPEG {version},18 JP2/630 pixels, 1/4/8/12/16-bit mixed unsigned CMYK and signed/unsigned alpha; independent integer rational RGB and exact PNG/COLR/CDEF; float error <=6e-8. ICC/print color management and native CMYK editing remain separate.')
