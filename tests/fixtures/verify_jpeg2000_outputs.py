"""Independent C OpenJPEG decoding and JP2 boxes/J2K SIZ/COD markers."""
from pathlib import Path
import json,struct
import os
os.environ["OPENCV_LOG_LEVEL"]="SILENT"
import cv2,numpy as np
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/jpeg2000-output'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==216
pixels=0;lossy_difference=0;metrics={};sizes={}
def boxes(data):
    at=0;result={}
    while at<len(data):
        n,kind=struct.unpack_from('>I4s',data,at);assert n>=8 and at+n<=len(data)
        result[kind]=data[at+8:at+n];at+=n
    assert at==len(data);return result
for case in cases:
    path=OUT/case['file'];data=path.read_bytes();channels=case['channels'];w,h=case['size'];depth=case['depth']
    if path.suffix=='.jp2':
        b=boxes(data);assert b[b'jP  ']==b'\r\n\x87\n';assert b[b'ftyp'][:4]==b'jp2 '
        header=boxes(b[b'jp2h']);assert struct.unpack('>IIH',header[b'ihdr'][:10])==(h,w,channels)
        assert header[b'colr'][:3]==bytes([1,0,0]) and struct.unpack('>I',header[b'colr'][3:])[0]==16
        if channels==4:
            definitions=header[b'cdef'];count=struct.unpack_from('>H',definitions)[0];entries=[struct.unpack_from('>HHH',definitions,2+i*6) for i in range(count)]
            assert (3,1,0) in entries
        data=b[b'jp2c']
    assert data[:2]==b'\xffO';at=2;markers={}
    while data[at:at+2]!=b'\xff\x90':
        marker=data[at:at+2];n=struct.unpack_from('>H',data,at+2)[0];assert n>=2;markers[marker]=data[at+4:at+2+n];at+=2+n
    size=markers[b'\xffQ'];assert struct.unpack_from('>II',size,2)==(w,h);assert struct.unpack_from('>H',size,34)[0]==channels
    assert list(size[36::3])==[depth-1]*channels
    cod=markers[b'\xffR'];assert cod[9]==(1 if case['lossless'] else 0);assert cod[4]==1
    decoded=cv2.imdecode(np.frombuffer(path.read_bytes(),dtype='u1'),cv2.IMREAD_UNCHANGED);assert decoded is not None
    assert decoded.shape==(h,w,channels);assert decoded.dtype==np.dtype('u1' if depth==8 else 'u2')
    decoded=decoded[:,:,[2,1,0]+([3] if channels==4 else [])].astype('i4')
    native=np.fromfile(OUT/case['decoded'],dtype='<i4').reshape(h,w,channels)
    difference=np.max(np.abs(decoded-native));assert difference==0,(case['file'],difference)
    source=np.fromfile(OUT/case['source'],dtype='<f4').reshape(h,w,4)[:,:,:channels];maximum=np.float32((1<<depth)-1)
    expected=np.floor(np.clip(source,0,1)*maximum+np.float32(.5)).astype('i4')
    if case['lossless']:np.testing.assert_array_equal(decoded,expected,err_msg=case['file'])
    else:lossy_difference=max(lossy_difference,int(difference))
    if w==257 and depth==8 and 'src8-depth8-' in case['file'] and path.suffix=='.jp2':
        key='lossless' if case['lossless'] else case['ratio'];sizes[key]=path.stat().st_size;metrics[key]=np.mean(((decoded.astype('f8')-expected)/255)**2)
    pixels+=w*h
assert sizes[8]<sizes[2]<sizes['lossless'];assert metrics[8]>metrics[2]>metrics['lossless']==0
print(f'JPEG2000: {len(cases)} C OpenJPEG decoded JP2/J2K cases/{pixels} pixels; exact lossless 8/16-bit RGB/RGBA, tiny/single-column, HDR quantization, JP2 sRGB/alpha and SIZ/COD markers, lossy color/alpha with maximum Rust-vs-C rounding difference {lossy_difference} units; sizes {sizes}, MSE {metrics}')
