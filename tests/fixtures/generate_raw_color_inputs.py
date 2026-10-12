"""Own 3/4-channel single-illuminant linear/CFA DNG encoder."""
from pathlib import Path
import struct,json,hashlib
import numpy as np
ROOT=Path(__file__).resolve().parent;OUT=ROOT/'raw-color';OUT.mkdir(exist_ok=True)
manifest=[]
def encode(channels,depth,illuminant,cfa=False,planes=None):
    template='cfa-16-o1.dng' if cfa else ('linear-16-o6-crop.dng' if depth==16 else 'linear-32-o1.dng')
    data=(ROOT/'raw'/template).read_bytes();end='<' if data[:2]==b'II' else '>';pack=lambda fmt,*v:struct.pack(end+fmt,*v)
    at=struct.unpack_from(end+'I',data,4)[0];n=struct.unpack_from(end+'H',data,at)[0];tags={};sizes={1:1,2:1,3:2,4:4,5:8,10:8}
    for i in range(n):
        p=at+2+i*12;tag,typ,count=struct.unpack_from(end+'HHI',data,p);size=sizes[typ]*count;start=p+8 if size<=4 else struct.unpack_from(end+'I',data,p+8)[0];tags[tag]=(typ,count,data[start:start+size])
    cpp=1 if cfa else channels;black=64 if depth==16 else 0;white=4095 if depth==16 else 1;w,h=12,10
    if cfa:
        constant=np.array([.2,.3,.1,.25]);normalized=np.array([[constant[(y%2)*2+x%2] for x in range(w)] for y in range(h)])
    else:normalized=np.array([[[.10+x*.022,.15+y*.027,.20+(x+y)*.008,.25+x*.011][:channels] for x in range(w)] for y in range(h)])
    sensor=np.rint(normalized*(white-black)+black).astype('u2') if depth==16 else normalized.astype('f4');pixels=sensor.astype(end+('u2' if depth==16 else 'f4')).tobytes()
    matrix=np.eye(3) if channels==3 else np.vstack([np.eye(3),[.2,.4,.3]])
    tags[258]=(3,cpp,pack('H'*cpp,*([depth]*cpp)));tags[277]=(3,1,pack('H',cpp));tags[279]=(4,1,pack('I',len(pixels)));tags[339]=(3,cpp,pack('H'*cpp,*([1 if depth==16 else 3]*cpp)))
    tags[50714]=(5,cpp,b''.join(pack('II',black,1) for _ in range(cpp)));tags[50717]=(4,cpp,pack('I'*cpp,*([white]*cpp)));tags[50721]=(10,channels*3,b''.join(pack('ii',round(float(v)*10000000),10000000) for v in matrix.ravel()));tags[50778]=(3,1,pack('H',illuminant))
    if channels==4 and depth==16:
        neutral=[1,2,1,2];tags[50728]=(3,4,pack('HHHH',*neutral));wb=[1,.5,1,.5]
    else:
        neutral=[(1,2),(1,1),(1,4),(1,2)][:channels];tags[50728]=(5,channels,b''.join(pack('II',*v) for v in neutral));wb=[2,1,4,2][:channels]
    if cfa:tags[33422]=(1,4,bytes([0,1,2,3]));tags[50710]=(1,4,bytes(planes or [0,1,2,3]))
    offset=8+2+12*len(tags)+4;entries=[];extra=bytearray()
    for tag,(typ,count,payload) in sorted(tags.items()):
        if tag==273:continue
        if len(payload)>4:
            entries.append((tag,pack('HHII',tag,typ,count,offset+len(extra))));extra.extend(payload)
            if len(extra)%2:extra.append(0)
        else:entries.append((tag,pack('HHI',tag,typ,count)+payload.ljust(4,b'\0')))
    entries.append((273,pack('HHII',273,4,1,offset+len(extra))))
    encoded=(b'II' if end=='<' else b'MM')+pack('HI',42,8)+pack('H',len(tags))+b''.join(v for _,v in sorted(entries))+pack('I',0)+extra+pixels
    name=f'{"cfa" if cfa else "linear"}-{channels}c-{depth}-ill{illuminant}'+(''.join(map(str,planes)) if planes else '')+'.dng';(OUT/name).write_bytes(encoded)
    manifest.append({'file':name,'channels':channels,'depth':depth,'illuminant':illuminant,'cfa':cfa,'planes':planes or ([0,1,2,3] if cfa else None),'source_sha256':hashlib.sha256(encoded).hexdigest(),'wb':wb})
for illuminant in [21,17,18,19,23,20,22,1,4]:
    for channels in [3,4]:
        for depth in [16,32]:encode(channels,depth,illuminant)
for illuminant in [21,17,23]:encode(4,16,illuminant,True)
for planes in [[3,4,5,1],[2,1,0,3]]:
    for illuminant in [21,17,23]:encode(4,16,illuminant,True,planes)
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2));(OUT/'README.md').write_text('Own 45 single-illuminant sensor DNGs: 3/4 camera channels, 16/32-bit linear, constant RGB/Cyan, CMYG and reordered four-color CFA, byte order/crop/orientation, SHORT/RATIONAL AsShotNeutral and known 3x3/4x3 matrices. Independent encoder generate_raw_color_inputs.py. Synthetic CFA interiors are compared; real cameras/edge quality/dual profiles/ICC remain unverified.\n')
print(f'Encoded {len(manifest)} own 3/4-channel single-illuminant DNG fixtures')
