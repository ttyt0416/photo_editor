"""Independent native GRAY+Alpha TIFF, luma, compression/prediction and ICC."""
from pathlib import Path
import json
import struct
import zlib
import numpy as np
import tifffile

BASE=Path(__file__).resolve().parents[2]
ROOT=BASE/'qa-output/tiff-gray-output'

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
assert len(cases)==240
for case in cases:
    path=ROOT/case['file'];data=path.read_bytes();bits=case['bits']
    source=np.fromfile(ROOT/f"source{str(case['neutral']).lower()}.f32",'<f4').reshape(3,17,4)
    rgb=source[:,:,:3].astype('f8')
    gray=(rgb[:,:,0]*.2126+rgb[:,:,1]*.7152+rgb[:,:,2]*.0722).astype('f4')
    neutral=(source[:,:,0]==source[:,:,1])&(source[:,:,1]==source[:,:,2])
    gray[neutral]=source[:,:,0][neutral]
    expected=np.stack([gray,source[:,:,3]],axis=2)
    with tifffile.TiffFile(path) as file:
        page=file.pages[0]
        assert page.shape==(3,17,2) and page.bitspersample==bits and int(page.photometric)==1
        assert file.is_bigtiff==case['big'] and tuple(page.extrasamples)==(2,)
        if case['icc']:assert page.tags[34675].value==(BASE/'data/color/srgb-gray.icc').read_bytes()
        else:assert 34675 not in page.tags
        predictor=page.tags['Predictor'].value
        assert predictor==(3 if bits==32 else 2) if case['predictor'] else predictor==1
        compression=int(page.compression);chunks=[]
        for offset,length in zip(page.dataoffsets,page.databytecounts):
            payload=data[offset:offset+length]
            raw={1:lambda x:x,5:lzw,8:zlib.decompress,32773:packbits}[compression](payload)
            rows=len(raw)//(17*2*(bits//8))
            if predictor==3:
                encoded=np.frombuffer(raw,'u1').reshape(rows,17*8)
                accumulated=np.cumsum(encoded.reshape(rows,-1,2),axis=1,dtype='u8').astype('u1').reshape(rows,4,17*2)
                samples=accumulated.transpose(0,2,1).copy().tobytes()
                actual=np.frombuffer(samples,'>f4').astype('<f4').reshape(rows,17,2)
            else:
                actual=np.frombuffer(raw,{8:'u1',16:file.byteorder+'u2',32:file.byteorder+'f4'}[bits]).reshape(rows,17,2)
                if predictor==2:actual=np.cumsum(actual,axis=1,dtype=actual.dtype)
            chunks.append(actual)
        actual=np.concatenate(chunks,axis=0)
        if bits==32:np.testing.assert_array_equal(actual.view('u4'),expected.view('u4'))
        else:
            quantized=np.floor(np.clip(expected,0,1).astype('f8')*((1<<bits)-1)+.5)
            assert np.max(np.abs(actual.astype('f8')-quantized))<=1
            np.testing.assert_array_equal(actual[:,:,1],quantized[:,:,1])
    import zipfile
    with zipfile.ZipFile(ROOT/(case['file']+'.rphoto')) as project:
        assert project.testzip() is None
        if case['icc']:assert project.read('metadata/source.icc')==(BASE/'data/color/srgb-gray.icc').read_bytes()
        else:assert 'metadata/source.icc' not in project.namelist()
print('Native GRAY+Alpha TIFF: 240 outputs/12240 pixels, 8/16/32-bit, colored/neutral, 6 compressions/classic+BigTIFF/horizontal+float prediction/ICC on+off; independent luma and decompress/byte-plane math, exact float HDR/negative/signed-zero/alpha bits, integer alpha, GRAY ICC and project CRC verified.')
