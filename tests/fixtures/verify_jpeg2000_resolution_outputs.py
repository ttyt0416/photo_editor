"""Independent exact resolution fractions, header containment and unchanged coding."""
from pathlib import Path
from fractions import Fraction
import json,struct
import numpy as np
from jpeg2000_c_reference import components,version
OUT=Path(__file__).resolve().parents[2]/'qa-output/jpeg2000-output/resolution'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==40

def boxes(data):
    at=0; result={}
    while at<len(data):
        n,kind=struct.unpack_from('>I4s',data,at);assert n>=8 and at+n<=len(data) and kind not in result
        result[kind]=data[at+8:at+n];at+=n
    assert at==len(data);return result

baselines={}; raw_baselines={}
for case in cases:
    path=OUT/case['file'];data=path.read_bytes();b=boxes(data);header=boxes(b[b'jp2h'])
    assert struct.unpack('>IIH',header[b'ihdr'][:10])==(11,17,case['channels'])
    if case['enabled']:
        resolution=boxes(header[b'res ']);assert list(resolution)==[b'resd']
        vn,vd,hn,hd,ve,he=struct.unpack('>HHHHbb',resolution[b'resd'])
        assert vd>0 and hd>0
        vertical=Fraction(vn,vd)*10**ve*Fraction(254,10000)
        horizontal=Fraction(hn,hd)*10**he*Fraction(254,10000)
        assert vertical==horizontal==case['ppi']
    else:assert b'res ' not in header
    key=(case['gray'],case['lossless']);baseline=baselines.setdefault(key,b[b'jp2c'])
    assert b[b'jp2c']==baseline,(case['file'],'resolution changed compressed pixels')
    raw=(OUT/(case['file']+'.j2k')).read_bytes()
    assert raw==raw_baselines.setdefault(key,raw)
    native=np.fromfile(OUT/case['decoded'],dtype='<i4').reshape(11,17,case['channels'])
    np.testing.assert_array_equal(components(path),native,err_msg=case['file'])
assert (OUT/'invalid.jp2').read_bytes()==b'original'
print(f'JP2 resolution: {len(cases)} independent C OpenJPEG {version} exact 16-bit RGB/gray-alpha reads, exact ppi fractions 1/72/300/9600/65535, res /resd containment, disabled metadata, unchanged JP2 codestream with metadata on/off or changing ppi values, raw J2K ignores metadata, original invalid output preserved')
