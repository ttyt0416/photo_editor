"""Independent C decoder and full SIZ/COD/SOT tile traversal, including edges."""
from pathlib import Path
import json,struct
import numpy as np
from jpeg2000_c_reference import components,version
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/jpeg2000-output/tiled'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==128
pixels=0;parts=0;small_edges=0
for case in cases:
    path=OUT/case['file'];data=path.read_bytes();w,h=case['size'];tile=case['tile'];depth=case['depth'];channels=case['channels']
    if path.suffix=='.jp2':
        at=0; found=None
        while at<len(data):
            n,kind=struct.unpack_from('>I4s',data,at);assert n>=8 and at+n<=len(data)
            if kind==b'jp2c':found=data[at+8:at+n]
            at+=n
        assert at==len(data) and found is not None;data=found
    assert data[:2]==b'\xffO';at=2;markers={}
    while data[at:at+2]!=b'\xff\x90':
        n=struct.unpack_from('>H',data,at+2)[0];assert n>=2 and at+2+n<=len(data)
        markers[data[at:at+2]]=data[at+4:at+2+n];at+=2+n
    siz=markers[b'\xffQ'];assert struct.unpack_from('>IIIIIIII',siz,2)==(w,h,0,0,tile,tile,0,0)
    assert struct.unpack_from('>H',siz,34)[0]==channels and list(siz[36::3])==[depth-1]*channels
    cod=markers[b'\xffR'];assert cod[1]==3 and struct.unpack_from('>H',cod,2)[0]==4
    assert cod[4]==(0 if case['gray'] else 1) and cod[9]==(1 if case['lossless'] else 0)
    ids=[]
    while data[at:at+2]!=b'\xff\xd9':
        assert data[at:at+2]==b'\xff\x90' and struct.unpack_from('>H',data,at+2)[0]==10
        index,length,part,count=struct.unpack_from('>HIBB',data,at+4)
        assert length>=14 and at+length<=len(data) and part==0
        ids.append(index);at+=length
    assert at+2==len(data);nx=(w+tile-1)//tile;ny=(h+tile-1)//tile
    assert sorted(ids)==list(range(nx*ny));parts+=len(ids)
    if min(w%tile or tile,h%tile or tile)==1:small_edges+=1
    source=np.fromfile(OUT/case['source'],dtype='<f4').reshape(h,w,4)
    if case['gray']:
        gray=(source[:,:,0].astype('f8')*.2126+source[:,:,1].astype('f8')*.7152+source[:,:,2].astype('f8')*.0722).astype('f4')
        neutral=(source[:,:,0]==source[:,:,1])&(source[:,:,1]==source[:,:,2]);gray[neutral]=source[:,:,0][neutral]
        source=np.stack([gray,source[:,:,3]],axis=2)
    expected=np.floor(np.clip(source,0,1)*np.float32((1<<depth)-1)+np.float32(.5)).astype('i4')
    for sample in case['decoded']:
        native=np.fromfile(OUT/sample['file'],dtype='<i4').reshape(h,w,channels)
        decoded=components(path,sample['layers'])
        np.testing.assert_array_equal(decoded,native,err_msg=case['file']+str(sample['layers']))
        if sample['layers']==0 and case['lossless']:np.testing.assert_array_equal(decoded,expected,err_msg=case['file'])
        pixels+=w*h
print(f'Tiled JPEG2000: C OpenJPEG {version}, {len(cases)} files/{len(cases)*2} final/first-quality results/{pixels} pixels, {parts} SOT tile parts; all exact RGB/gray alpha 8/16-bit lossy/lossless comparisons, {small_edges} one-pixel edge cases, SIZ/COD/SOT tile grids and exact final lossless source')
