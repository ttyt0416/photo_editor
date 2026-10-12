"""Own dual-illuminant DNG encoder from our synthetic single-profile files."""
from pathlib import Path
import struct,json,hashlib
import numpy as np
ROOT=Path(__file__).resolve().parent;OUT=ROOT/'raw-dual';OUT.mkdir(exist_ok=True)
manifest=[]
def encode(channels,depth,lights,reverse=False,cfa=False):
    template=f'cfa-4c-16-ill213451.dng' if cfa else f'linear-{channels}c-{depth}-ill21.dng'
    data=(ROOT/'raw-color'/template).read_bytes();end='<' if data[:2]==b'II' else '>';pack=lambda f,*v:struct.pack(end+f,*v)
    n=struct.unpack_from(end+'H',data,8)[0];tags={};sizes={1:1,2:1,3:2,4:4,5:8,10:8}
    for i in range(n):
        p=10+i*12;tag,typ,count=struct.unpack_from(end+'HHI',data,p);size=sizes[typ]*count;start=p+8 if size<=4 else struct.unpack_from(end+'I',data,p+8)[0];tags[tag]=(typ,count,data[start:start+size])
    start=struct.unpack(end+'I',tags[273][2])[0];length=struct.unpack(end+'I',tags[279][2])[0];pixels=data[start:start+length]
    m1=np.array([[.82,.12,.06],[.08,.86,.06],[.04,.18,.78]]);m2=np.array([[.97,.02,.01],[.02,.96,.02],[.03,.02,.95]])
    if channels==4:m1=np.vstack([m1,[.18,.52,.30]]);m2=np.vstack([m2,[.27,.35,.38]])
    matrices=[m1,m2];lights=list(lights)
    if reverse:matrices.reverse();lights.reverse()
    for slot,(matrix,light) in enumerate(zip(matrices,lights)):
        tags[50721+slot]=(10,channels*3,b''.join(pack('ii',round(float(v)*10000000),10000000) for v in matrix.ravel()))
        tags[50778+slot]=(3,1,pack('H',light))
    offset=8+2+12*len(tags)+4;entries=[];extra=bytearray()
    for tag,(typ,count,payload) in sorted(tags.items()):
        if tag==273:continue
        if len(payload)>4:
            entries.append((tag,pack('HHII',tag,typ,count,offset+len(extra))));extra.extend(payload)
            if len(extra)%2:extra.append(0)
        else:entries.append((tag,pack('HHI',tag,typ,count)+payload.ljust(4,b'\0')))
    entries.append((273,pack('HHII',273,4,1,offset+len(extra))))
    encoded=data[:4]+pack('I',8)+pack('H',len(tags))+b''.join(v for _,v in sorted(entries))+pack('I',0)+extra+pixels
    name=f'{"cfa" if cfa else "linear"}-{channels}c-{depth}-ill{lights[0]}-{lights[1]}.dng';(OUT/name).write_bytes(encoded)
    manifest.append({'file':name,'channels':channels,'depth':depth,'lights':lights,'cfa':cfa,'source_sha256':hashlib.sha256(encoded).hexdigest()})
for lights in [(17,21),(23,22)]:
    for channels in [3,4]:
        for depth in [16,32]:
            for reverse in [False,True]:encode(channels,depth,lights,reverse)
for reverse in [False,True]:encode(4,16,(17,21),reverse,True)
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2))
(OUT/'README.md').write_text('Own 18 dual-illuminant DNGs: A/D65 and D50/D75, reversed profile slots, known unequal 3x3/4x3 matrices, linear16/32 and constant CMYG CFA, two byte orders/crop/orientation and four-channel WB. Synthetic profiles are not evidence of real-camera/ICC/complete DNG support.\n')
print(f'Encoded {len(manifest)} own dual-illuminant fixtures')
