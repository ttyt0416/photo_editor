"""Independent C component planes, vector chroma expansion and full-range color."""
from pathlib import Path
import json,os
os.environ['OPENCV_LOG_LEVEL']='SILENT'
import numpy as np,cv2
from jpeg2000_c_reference import component_planes,version
OUT=Path(__file__).resolve().parents[2]/'qa-output/jpeg2000-input/sampling'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==48
pixels=0
for case in cases:
    path=OUT/case['file'];planes=component_planes(path);w,h=case['size'];dy=case['dy'];precision=case['precision'];limit=(1<<precision)-1;center=1<<(precision-1)
    assert planes[0].shape==(h,w);assert planes[1].shape==planes[2].shape==((h+dy-1)//dy,(w+1)//2)
    chroma=[np.repeat(np.repeat(plane,dy,axis=0),2,axis=1)[:h,:w].astype('f8')-center for plane in planes[1:3]]
    cb,cr=chroma;y=planes[0].astype('f8')
    rgb=np.clip(y[:,:,None]+np.trunc(np.stack([1.402*cr,-(.344*cb+.714*cr),1.772*cb],axis=2)),0,limit).astype('i8')
    alpha=planes[3].astype('i8') if case['alpha'] else np.full((h,w),limit)
    rgba=np.concatenate([rgb,alpha[:,:,None]],axis=2)
    decoded=np.fromfile(OUT/case['decoded'],dtype='<f4').reshape(h,w,4)
    np.testing.assert_allclose(decoded,rgba.astype('f4')/np.float32(limit),rtol=0,atol=6e-8,err_msg=case['file'])
    rgba=rgba.copy();rgba[:,:,:3][rgba[:,:,3]==0]=0
    png=cv2.imdecode(np.frombuffer((OUT/(case['file']+'.png')).read_bytes(),'u1'),cv2.IMREAD_UNCHANGED)
    assert png.shape==(h,w,4);np.testing.assert_array_equal(png[:,:,[2,1,0,3]],rgba.astype('u1' if precision==8 else 'u2'))
    pixels+=w*h
boxes=OUT.parent/'sampling-boxes'
normal=component_planes(boxes/'normal.jp2')
for name in ['extendedfalse.jp2','extendedtrue.jp2']:
    other=component_planes(boxes/name)
    for expected,actual in zip(normal,other):np.testing.assert_array_equal(expected,actual)
print(f'sYCC sampling: C OpenJPEG {version}, {len(cases)} JP2/{pixels} pixels, 4:2:2/4:2:0 8/16-bit RGB/alpha, odd/tiny/even dimensions and single rows/columns; vector chroma/color math, exact PNG, full C component agreement for XLBOX/to-EOF boxes, float error <=6e-8')
