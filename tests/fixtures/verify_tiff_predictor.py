"""Independent Predictor 3 byte-plane and metadata checks (no Rust decoder involved)."""
from pathlib import Path
import struct, zlib, json
import numpy as np
import tifffile
ROOT = Path(__file__).resolve().parents[2] / "qa-output/tiff-predictor"
manifest=json.loads((ROOT/"manifest.json").read_text())
def lzw(data):
    bit=0;width=9;table=[bytes([i]) for i in range(256)]+[b'',b''];previous=None;out=bytearray()
    def read(n):
        nonlocal bit
        assert bit+n<=8*len(data);value=0
        for _ in range(n): value=(value<<1)|((data[bit//8]>>(7-bit%8))&1);bit+=1
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
predicted=plain=0
for name,info in manifest.items():
    path=ROOT/name;data=path.read_bytes();w=info['width'];h=info['height']
    source=np.frombuffer((ROOT/info['source']).read_bytes(),dtype='<f4').reshape(h,w,4)
    for y in range(h):
        for x in range(w):
            alpha=.75 if w==1 else 0 if x==0 else .5 if x==1 else 1
            expected=[1.25+np.float32(x)/np.float32(19),-.125+np.float32(y)/np.float32(37),0,alpha] if alpha>0 else [0]*4
            assert np.max(np.abs(source[y,x]-expected))<1e-6
    with tifffile.TiffFile(path) as file:
        page=file.pages[0];tags=page.tags
        assert file.is_bigtiff==info['big'] and len(file.pages)==1
        assert page.shape==(h,w,4) and page.bitspersample==32
        assert tags['Predictor'].value==3 and tags['SampleFormat'].value==(3,3,3,3)
        assert tuple(tags['ExtraSamples'].value)==(2,)
        compression=int(tags['Compression'].value);assert compression in (5,8)
        chunks=[]
        for offset,size in zip(page.dataoffsets,page.databytecounts):
            block=data[offset:offset+size];raw=lzw(block) if compression==5 else zlib.decompress(block)
            assert len(raw)%(w*16)==0
            rows=len(raw)//(w*16)
            encoded=np.frombuffer(raw,dtype='u1').reshape(rows,w*16)
            # Accumulation follows the channel stride over the entire shuffled row, including plane boundaries.
            accumulated=np.cumsum(encoded.reshape(rows,-1,4),axis=1,dtype='u8').astype('u1').reshape(rows,4,w*4)
            samples=accumulated.transpose(0,2,1).copy().tobytes()
            chunks.append(np.frombuffer(samples,dtype='>f4').astype('<f4').reshape(rows,w,4))
        actual=np.concatenate(chunks,axis=0)
        assert actual.shape==source.shape and np.array_equal(actual.view('<u4'),source.view('<u4')),name
        if w==317:assert len(chunks)>1
    if w==317:
        predicted+=path.stat().st_size
        plain+=(ROOT/(path.stem+'-plain.tiff')).stat().st_size
assert len(manifest)==24 and predicted<plain
print(f'TIFF Predictor 3: 24 LZW/Deflate/classic/BigTIFF combinations, single-pixel and multiple strips, exact HDR/negative/alpha bits verified')
print(f'Large smooth fixture bytes: prediction {predicted}, ordinary compression {plain}')
