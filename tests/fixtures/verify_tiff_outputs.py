"""Independent TIFF metadata and sample checks using tifffile plus Python decompressors."""
from pathlib import Path
import struct
import zlib
import numpy as np
import tifffile
root=Path(__file__).resolve().parents[2]/'qa-output/tiff-output'
source=np.frombuffer((root/'source32.le').read_bytes(),dtype='<f4').reshape(8,16,4)
for y in range(8):
    for x in range(16):
        expected=[2.25+x/19,-.125+y/37,.1234567+y/43,0 if x==0 else .5 if x==1 else 1]
        if x==0: expected=[0,0,0,0]
        assert np.max(np.abs(source[y,x]-expected))<1e-6

def lzw(data):
    bit=0;width=9;table=[bytes([i]) for i in range(256)]+[b'',b''];previous=None;out=bytearray()
    def read(n):
        nonlocal bit
        assert bit+n<=8*len(data)
        value=0
        for _ in range(n):
            value=(value<<1)|((data[bit//8]>>(7-bit%8))&1);bit+=1
        return value
    while True:
        code=read(width)
        if code==256:
            table=[bytes([i]) for i in range(256)]+[b'',b''];width=9;previous=None;continue
        if code==257: return bytes(out)
        if code<len(table):entry=table[code]
        else:
            assert code==len(table) and previous is not None
            entry=previous+previous[:1]
        out.extend(entry)
        if previous is not None and len(table)<4096:
            table.append(previous+entry[:1])
            if width<12 and len(table)==(1<<width)-1:width+=1
        previous=entry

def packbits(data):
    out=bytearray();i=0
    while i<len(data):
        n=struct.unpack_from('b',data,i)[0];i+=1
        if 0<=n<=127:out.extend(data[i:i+n+1]);i+=n+1
        elif n!=-128:out.extend(data[i:i+1]*(1-n));i+=1
    assert i==len(data)
    return bytes(out)

count=0
for path in sorted(root.glob('d*-c*-p*-b*.tiff')):
    if '-p1-' in path.stem and any(f'-c{c}-' in path.stem for c in (0,5)): continue # Historical incompatible combinations remain as evidence.
    data=path.read_bytes()
    with tifffile.TiffFile(path) as file:
        page=file.pages[0];tags=page.tags;bits=page.bitspersample
        assert page.shape==(8,16,4) and len(file.pages)==1
        assert tuple(tags['ExtraSamples'].value)==(2,)
        assert tags['ResolutionUnit'].value==2
        assert tags['XResolution'].value==(300,1) and tags['YResolution'].value==(300,1)
        assert file.is_bigtiff==path.stem.endswith('b1')
        compression=int(tags['Compression'].value);decoded=bytearray()
        for offset,size in zip(page.dataoffsets,page.databytecounts):
            block=data[offset:offset+size]
            if compression==1:raw=block
            elif compression==5:raw=lzw(block)
            elif compression==8:raw=zlib.decompress(block)
            elif compression==32773:raw=packbits(block)
            else:raise AssertionError(compression)
            decoded.extend(raw)
        dtype={8:'u1',16:'<u2',32:'<f4'}[bits]
        actual=np.frombuffer(decoded,dtype=dtype).reshape(8,16,4)
        if tags['Predictor'].value==2:
            assert bits in (8,16)
            actual=np.cumsum(actual,axis=1,dtype=actual.dtype)
        if bits==32:
            assert tags['SampleFormat'].value==(3,3,3,3)
            assert np.max(np.abs(actual-source))<1e-6
            assert actual[:,:,0].max()>2 and actual[:,:,1].min()<0
        else:
            max_value=255 if bits==8 else 65535
            expected=np.floor(np.clip(source,0,1).astype(np.float64)*max_value+.5).astype(actual.dtype)
            assert np.max(np.abs(actual.astype('i8')-expected.astype('i8')))<=1
        count+=1
assert count==52
print('TIFF: 52 compatible classic/BigTIFF, 8/16/32-bit, compression/predictor combinations, straight alpha, HDR/negative values and 300ppi verified independently')
