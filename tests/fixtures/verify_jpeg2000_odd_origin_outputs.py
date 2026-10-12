"""Independent C component planes, vector chroma expansion and full-range color."""
from pathlib import Path
import json,os,struct
os.environ['OPENCV_LOG_LEVEL']='SILENT'
import numpy as np,cv2
from jpeg2000_c_reference import component_planes,version
OUT=Path(__file__).resolve().parents[2]/'qa-output/jpeg2000-input/sampling-odd-origins'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==216
pixels=0
for case in cases:
    path=OUT/case['file'];w,h=case['size'];dy=case['dy'];precision=case['precision'];x0,y0=case['origin'];assert x0%2 or y0%dy;limit=(1<<precision)-1;center=1<<(precision-1)
    encoded=path.read_bytes();at=0
    while encoded[at+4:at+8]!=b'jp2c': at+=struct.unpack_from('>I',encoded,at)[0]
    stream=encoded[at+8:];assert stream[:4]==bytes.fromhex('ff4fff51')
    xs,ys,ox,oy=struct.unpack_from('>IIII',stream,8)
    assert (xs,ys,ox,oy)==(x0+w,y0+h,x0,y0)
    count=struct.unpack_from('>H',stream,40)[0];assert count==3+int(case['alpha'])
    assert [tuple(stream[43+3*c:45+3*c]) for c in range(count)]==[(1,1),(2,dy),(2,dy)]+([(1,1)] if case['alpha'] else [])
    if case['empty']:
        try: component_planes(path)
        except AssertionError: continue
        raise AssertionError(f"C decoder unexpectedly accepted empty component: {path}")
    planes=component_planes(path)
    assert planes[0].shape==(h,w);assert planes[1].shape==planes[2].shape==(((y0+h+dy-1)//dy)-(y0+dy-1)//dy,((x0+w+1)//2)-(x0+1)//2)
    # Expand on the absolute reference grid; absent leading samples are raw 0.
    expanded=[]
    offx=x0%2;offy=y0%dy
    for plane in planes[1:3]:
        whole=np.zeros((h,w),dtype='f8')
        patch=np.repeat(np.repeat(plane,dy,axis=0),2,axis=1)
        whole[offy:,offx:]=patch[:h-offy,:w-offx]
        expanded.append(whole-center)
    chroma=expanded
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
print(f'sYCC odd origins: C OpenJPEG {version}, {len(cases)} JP2/132 readable/{pixels} pixels, 84 empty-component files consistently rejected by C and Rust, 4:2:2/4:2:0 8/16-bit RGB/alpha, odd/tiny/even dimensions and single rows/columns; vector chroma/color math, exact PNG, non-aligned SIZ/component coordinates and explicit empty-component codec errors, float error <=6e-8')
