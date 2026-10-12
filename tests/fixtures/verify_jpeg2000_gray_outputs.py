"""Independent FFmpeg JPEG2000 decoder; gray JP2 boxes and lossless source math."""
from pathlib import Path
import json, struct, subprocess, os
import numpy as np
from jpeg2000_c_reference import components as c_components, version as c_version
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'qa-output/jpeg2000-output/gray'
FFMPEG=Path('C:/ProgramData/chocolatey/bin/ffmpeg.exe')
cases=json.loads((OUT/'manifest.json').read_text()); assert len(cases)==96

def boxes(data):
    at=0; result={}
    while at<len(data):
        n,kind=struct.unpack_from('>I4s',data,at); assert n>=8 and at+n<=len(data)
        result[kind]=data[at+8:at+n]; at+=n
    assert at==len(data); return result

pixels=0; differences={}; lossless=0
for case in cases:
    path=OUT/case['file']; data=path.read_bytes(); channels=case['channels']; w,h=case['size']; depth=case['depth']
    if path.suffix=='.jp2':
        b=boxes(data); header=boxes(b[b'jp2h'])
        assert struct.unpack('>IIH',header[b'ihdr'][:10])==(h,w,channels)
        assert header[b'colr'][:3]==bytes([1,0,0]) and struct.unpack('>I',header[b'colr'][3:])[0]==17
        if channels==2:
            definitions=header[b'cdef']; count=struct.unpack_from('>H',definitions)[0]
            entries=[struct.unpack_from('>HHH',definitions,2+i*6) for i in range(count)]
            assert (1,1,0) in entries
        data=b[b'jp2c']
    assert data[:2]==b'\xffO'; at=2; markers={}
    while data[at:at+2]!=b'\xff\x90':
        marker=data[at:at+2]; n=struct.unpack_from('>H',data,at+2)[0]; assert n>=2
        markers[marker]=data[at+4:at+2+n]; at+=2+n
    siz=markers[b'\xffQ']; assert struct.unpack_from('>II',siz,2)==(w,h)
    assert struct.unpack_from('>H',siz,34)[0]==channels
    assert list(siz[36::3])==[depth-1]*channels
    cod=markers[b'\xffR']; assert cod[9]==(1 if case['lossless'] else 0) and cod[4]==0
    assert cod[1]==2 and struct.unpack_from('>H',cod,2)[0]==4
    fmt=('ya8' if depth==8 else 'ya16le') if channels==2 else ('gray' if depth==8 else 'gray16le')
    command=[str(FFMPEG),'-v','error','-threads','1','-c:v','jpeg2000','-i',str(path),'-frames:v','1','-c:v','rawvideo','-threads','1','-pix_fmt',fmt,'-f','rawvideo','pipe:1']
    result=subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,creationflags=0x08000000 if os.name=='nt' else 0,check=True)
    decoded=np.frombuffer(result.stdout,dtype='u1' if depth==8 else '<u2').astype('i4').reshape(h,w,channels)
    native=np.fromfile(OUT/case['decoded'],dtype='<i4').reshape(h,w,channels)
    np.testing.assert_array_equal(c_components(path),native,err_msg=case['file'])
    difference=int(np.max(np.abs(decoded-native)))
    differences[(depth,case['lossless'])]=max(differences.get((depth,case['lossless']),0),difference)
    # Independent FFmpeg 9/7 reconstruction uses its own arithmetic. Exact
    # reversible results remain mandatory; recorded irreversible corpus bounds
    # are 1/255 (8-bit) and 3/65535 (16-bit), not an ISO conformance claim.
    assert difference <= (0 if case['lossless'] else (1 if depth==8 else 3)), (case['file'],difference)

    source=np.fromfile(OUT/case['source'],dtype='<f4').reshape(h,w,4)
    gray=(source[:,:,0].astype('f8')*.2126+source[:,:,1].astype('f8')*.7152+source[:,:,2].astype('f8')*.0722).astype('f4')
    neutral=(source[:,:,0]==source[:,:,1])&(source[:,:,1]==source[:,:,2]); gray[neutral]=source[:,:,0][neutral]
    values=np.stack([gray,source[:,:,3]],axis=2) if channels==2 else gray[:,:,None]
    expected=np.floor(np.clip(values,0,1)*np.float32((1<<depth)-1)+np.float32(.5)).astype('i4')
    if case['lossless']:
        np.testing.assert_array_equal(decoded,expected,err_msg=case['file']); lossless+=1
    pixels+=w*h
print(f'Gray JPEG2000: C OpenJPEG {c_version} exact all lossy/lossless component checks;  {len(cases)} independent FFmpeg native decoded JP2/J2K cases/{pixels} pixels, {lossless} exact lossless source conversions; gray/alpha 8/16-bit, HDR clamping, neutral precision, 1-pixel rows/columns, COLR/CDEF/SIZ/COD; Rust vs FFmpeg maximum differences {differences}')
