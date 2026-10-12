"""Independent C decoding + vector sYCC full-range math and independent color math."""
from pathlib import Path
import json,os
os.environ['OPENCV_LOG_LEVEL']='SILENT'
import numpy as np,cv2
from jpeg2000_c_reference import components,version
OUT=Path(__file__).resolve().parents[2]/'qa-output/jpeg2000-input/sycc'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==6
for case in cases:
    path=OUT/case['file'];planes=components(path).astype('f8');precision=case['precision'];limit=(1<<precision)-1;center=1<<(precision-1);target=(1<<case['depth'])-1
    y=planes[:,:,0];cb=planes[:,:,1]-center;cr=planes[:,:,2]-center
    offsets=np.stack([1.402*cr,-(.344*cb+.714*cr),1.772*cb],axis=2)
    rgb=np.clip(y[:,:,None]+np.trunc(offsets),0,limit).astype('i8')
    alpha=planes[:,:,3].astype('i8') if case['alpha'] else np.full(y.shape,limit)
    rgba=np.concatenate([rgb,alpha[:,:,None]],axis=2)*target//limit
    decoded=np.fromfile(OUT/case['decoded'],dtype='<f4').reshape(5,7,4)
    np.testing.assert_allclose(decoded,rgba.astype('f4')/np.float32(target),rtol=0,atol=6e-8,err_msg=case['file'])
    # OpenCV maps enumerated sYCC through COLOR_YUV2BGR (different matrix),
    # so it is unsuitable as this color reference. Preserve full-precision
    # C decoded components and apply the independently vectorized sYCC math.
    if precision==8:
        from PIL import Image
        display=np.asarray(Image.open(path).convert('RGBA')).astype('i4')
        assert np.max(np.abs(display-rgba))<=1,case['file']
    rgba=rgba.copy();rgba[:,:,:3][rgba[:,:,3]==0]=0
    png=cv2.imdecode(np.frombuffer((OUT/(case['file']+'.png')).read_bytes(),'u1'),cv2.IMREAD_UNCHANGED)
    np.testing.assert_array_equal(png[:,:,[2,1,0,3]],rgba.astype('u1' if case['depth']==8 else 'u2'))
print(f'sYCC JPEG2000: C OpenJPEG {version}, 6 JP2/210 pixels, 8/12/16-bit RGB/alpha full range; independent vector matrix, 8-bit Pillow display within 1 unit, integer clipping/scaling, exact PNG, float error <=6e-8')
