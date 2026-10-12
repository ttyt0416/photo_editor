"""Independent C components, signed bias/clamp/full-range integer scaling and PNG."""
from pathlib import Path
import json
import numpy as np
from PIL import Image
from jpeg2000_c_reference import components,version
OUT=Path(__file__).resolve().parents[2]/'qa-output/jpeg2000-input'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==84
pixels=0
for case in cases:
    path=OUT/case['file'];native=components(path);w,h=case['size'];count=len(case['precisions']);target=(1<<case['depth'])-1
    expected=[]
    for channel,(precision,signed) in enumerate(zip(case['precisions'],case['signed'])):
        bias=1<<(precision-1) if signed else 0
        values=np.clip(native[:,:,channel].astype('i8')+bias,0,(1<<precision)-1)*target//((1<<precision)-1)
        expected.append(values)
    rgba=np.stack([expected[0],expected[0] if count<3 else expected[1],expected[0] if count<3 else expected[2],expected[1] if count==2 else expected[3] if count==4 else np.full((h,w),target)],axis=2)
    decoded=np.fromfile(OUT/case['decoded'],dtype='<f4').reshape(h,w,4)
    # All values derive from exact integer quantization; allow only the float
    # representation's final multiplication/division rounding, not codec error.
    np.testing.assert_allclose(decoded,rgba.astype('f4')/np.float32(target),rtol=0,atol=6e-8,err_msg=case['file'])
    # Export uses the document composite, whose fully transparent pixels are zero.
    rgba=rgba.copy();rgba[:,:,:3][rgba[:,:,3]==0]=0
    if case['depth']==8:
        np.testing.assert_array_equal(np.asarray(Image.open(OUT/(case['file']+'.png'))),rgba.astype('u1'))
    else:
        import cv2
        png=cv2.imdecode(np.frombuffer((OUT/(case['file']+'.png')).read_bytes(),'u1'),cv2.IMREAD_UNCHANGED)
        assert png.dtype==np.dtype('u2');np.testing.assert_array_equal(png[:,:,[2,1,0,3]],rgba.astype('u2'))
    pixels+=w*h
print(f'JPEG2000 input: C OpenJPEG {version}, {len(cases)} JP2/J2K/{pixels} pixels; signed/unsigned 1/4/8/12/16-bit 1-4 channels and mixed precision, integer endpoints/full-range bias scaling, exact 8/16-bit PNG exports, float error <=6e-8')
