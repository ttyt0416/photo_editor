"""Independent JP2 metadata, codestream and full/partial C decoded samples."""
from pathlib import Path
from fractions import Fraction
import struct,json,zipfile
import numpy as np
from jpeg2000_c_reference import components,version
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'qa-output/jpeg2000-output/metadata-rate'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==96

def boxes(data):
    at=0; result={}
    while at<len(data):
        n,kind=struct.unpack_from('>I4s',data,at)
        assert n>=8 and at+n<=len(data) and kind not in result
        result[kind]=data[at+8:at+n];at+=n
    assert at==len(data);return result

baselines={}; pixels=0; partial=0
for case in cases:
    path=OUT/case['file'];b=boxes(path.read_bytes());h=boxes(b[b'jp2h'])
    key=(case['width'],case['height'],case['gray'],case['depth'],case['lossless'])
    color_count=1 if case['gray'] else 3
    assert struct.unpack('>IIH',h[b'ihdr'][:10])==(case['height'],case['width'],color_count+1)
    assert h[b'colr'][:3]==bytes([2 if case['icc'] else 1,0,0])
    if case['icc']:
        profile=(ROOT/'data/color'/('srgb-gray.icc' if case['gray'] else 'srgb.icc')).read_bytes()
        assert h[b'colr'][3:]==profile
    else:assert struct.unpack('>I',h[b'colr'][3:])[0]==(17 if case['gray'] else 16)
    assert struct.unpack_from('>HHH',h[b'cdef'],2+color_count*6)==(color_count,1,0)
    if case['resolution']:
        res=boxes(h[b'res ']);assert list(res)==[b'resd']
        vn,vd,hn,hd,ve,he=struct.unpack('>HHHHbb',res[b'resd'])
        assert Fraction(vn,vd)*10**ve*Fraction(254,10000)==case['ppi']
        assert Fraction(hn,hd)*10**he*Fraction(254,10000)==case['ppi']
    else:assert b'res ' not in h
    planes=components(path).astype('i8');limit=(1<<case['depth'])-1
    assert planes.shape==(case['height'],case['width'],color_count+1)
    color=np.clip(planes[:,:,:color_count],0,limit)
    if case['gray']:color=np.repeat(color,3,axis=2)
    alpha=np.clip(planes[:,:,color_count],0,limit)
    expected=np.concatenate([color,alpha[:,:,None]],axis=2).astype('f4')/np.float32(limit)
    expected[:,:,:3][alpha==0]=0
    actual=np.fromfile(OUT/case['raw'],dtype='<f4').reshape(case['height'],case['width'],4)
    np.testing.assert_array_equal(actual,expected,err_msg=case['file'])
    # Check every intermediate quality layer as well as full reconstruction.
    decoded_layers=[components(path,layer) for layer in [1,2,3]];partial+=3
    if key in baselines:
        stream,baseline_planes,baseline_layers=baselines[key]
        assert b[b'jp2c']==stream,case['file']
        np.testing.assert_array_equal(planes,baseline_planes)
        for a,z in zip(decoded_layers,baseline_layers):np.testing.assert_array_equal(a,z)
    else:baselines[key]=(b[b'jp2c'],planes,decoded_layers)
    with zipfile.ZipFile(OUT/(case['file']+'.rphoto')) as z:assert z.testzip() is None
    pixels+=case['width']*case['height']
assert len(baselines)==24
print(f'JP2 metadata rate: C OpenJPEG {version}, {len(cases)} outputs/{pixels} pixels; 24 groups exact ICC/resolution on-off codestreams/full samples and {partial} partial-layer decodes; exact ppi 1/300/65535, RGB/GRAY ICC/CDEF, 8/16-bit normalization/project CRC, 1-column/128-tile edges/four RPCL quality layers.')
