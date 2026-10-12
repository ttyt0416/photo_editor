"""Independent auto TIFF precision, compression/prediction, ICC and float bits."""
from pathlib import Path
import json
import struct
import zlib
import numpy as np
import tifffile

BASE=Path(__file__).resolve().parents[2]
ROOT=BASE/'qa-output/tiff-auto'

def lzw(data):
    bit=0;width=9;table=[bytes([i]) for i in range(256)]+[b'',b''];previous=None;out=bytearray()
    def read(n):
        nonlocal bit
        assert bit+n<=8*len(data);value=0
        for _ in range(n):value=(value<<1)|((data[bit//8]>>(7-bit%8))&1);bit+=1
        return value
    while True:
        code=read(width)
        if code==256:table=[bytes([i]) for i in range(256)]+[b'',b''];width=9;previous=None;continue
        if code==257:return bytes(out)
        if code<len(table):entry=table[code]
        else:assert code==len(table) and previous is not None;entry=previous+previous[:1]
        out.extend(entry)
        if previous is not None and len(table)<4096:
            table.append(previous+entry[:1])
            if width<12 and len(table)==(1<<width)-1:width+=1
        previous=entry

def packbits(data):
    result=bytearray();position=0
    while position<len(data):
        count=struct.unpack_from('b',data,position)[0];position+=1
        if count>=0:result.extend(data[position:position+count+1]);position+=count+1
        elif count!=-128:result.extend(data[position:position+1]*(1-count));position+=1
    assert position==len(data)
    return bytes(result)

cases=json.loads((ROOT/'manifest.json').read_text())
assert len(cases)==24
for case in cases:
    path=ROOT/case['file'];data=path.read_bytes();bits=case['bits']
    source=np.fromfile(ROOT/f'source{bits}.f32','<f4').reshape(3,17,4)
    with tifffile.TiffFile(path) as file:
        page=file.pages[0]
        assert page.shape==(3,17,4) and page.bitspersample==bits
        assert file.is_bigtiff==case['big']
        assert page.tags[34675].value==(BASE/'data/color/srgb.icc').read_bytes()
        assert tuple(page.tags['ExtraSamples'].value)==(2,)
        predictor=page.tags['Predictor'].value
        compression=int(page.compression)
        chunks=[]
        for offset,length in zip(page.dataoffsets,page.databytecounts):
            payload=data[offset:offset+length]
            raw={1:lambda x:x,5:lzw,8:zlib.decompress,32773:packbits}[compression](payload)
            rows=len(raw)//(17*4*(bits//8))
            if predictor==3:
                assert bits==32
                encoded=np.frombuffer(raw,'u1').reshape(rows,17*16)
                accumulated=np.cumsum(encoded.reshape(rows,-1,4),axis=1,dtype='u8').astype('u1').reshape(rows,4,17*4)
                samples=accumulated.transpose(0,2,1).copy().tobytes()
                actual=np.frombuffer(samples,'>f4').astype('<f4').reshape(rows,17,4)
            else:
                actual=np.frombuffer(raw,{8:'u1',16:file.byteorder+'u2',32:file.byteorder+'f4'}[bits]).reshape(rows,17,4)
                if predictor==2:actual=np.cumsum(actual,axis=1,dtype=actual.dtype)
            chunks.append(actual)
        actual=np.concatenate(chunks,axis=0)
        if bits==32:
            np.testing.assert_array_equal(actual.view('u4'),source.view('u4'))
            assert actual[:,:,:3].min()<0 and actual[:,:,:3].max()>1
        else:
            expected=np.floor(np.clip(source,0,1).astype('f8')*((1<<bits)-1)+.5)
            assert np.max(np.abs(actual.astype('f8')-expected))<=1
            np.testing.assert_array_equal(actual[:,:,3],expected[:,:,3])
with tifffile.TiffFile(ROOT/'hidden32.tiff') as file:
    assert file.pages[0].bitspersample==32 and np.max(file.asarray())==0
print('Automatic TIFF: 24 8/16/32-bit outputs/1224 pixels + hidden32 layer; classic/BigTIFF/raw/LZW/Deflate/PackBits/horizontal+float prediction, independent decompression/byte planes, exact float HDR/negative/alpha bits, integer alpha, ICC and hidden precision verified.')
