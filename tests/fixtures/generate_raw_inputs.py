"""Own classic TIFF/DNG encoder; deterministic sensor samples, not embedded previews."""
from pathlib import Path
import struct,json,hashlib
import numpy as np
ROOT=Path(__file__).resolve().parent/'raw';ROOT.mkdir(exist_ok=True)
# DNG ColorMatrix maps XYZ to this synthetic camera's linear sRGB channels.
xyz=np.array([[.4124564,.3575761,.1804375],[.2126729,.7151522,.0721750],[.0193339,.1191920,.9503041]])
cam=np.linalg.inv(xyz)
manifest=[]
def encode(kind,depth,orientation=1,crop=False):
    w,h=12,10; little=orientation%2==1; end='<' if little else '>'
    pack=lambda fmt,*v:struct.pack(end+fmt,*v)
    black=64 if depth==16 else 0;white=4095 if depth==16 else 1
    rgb=np.array([[[.10+x*.022,.15+y*.027,.20+(x+y)*.008] for x in range(w)] for y in range(h)],dtype=np.float32)
    if kind=='cfa':
        # Uniform channel values let the whole interior demosaic be verified independently.
        rgb[:]=[.2,.3,.1]; pattern=[0,1,1,2]
        linear=np.array([[rgb[y,x,pattern[(y%2)*2+x%2]] for x in range(w)] for y in range(h)])
    elif kind=='mono':linear=rgb[:,:,1]
    else:linear=rgb
    sensor=np.rint(linear*(white-black)+black).astype(np.uint16) if depth==16 else linear.astype(np.float32)
    count=3 if kind=='linear' else 1
    data=sensor.astype(end+('u2' if depth==16 else 'f4')).tobytes()
    tags={256:(4,1,pack('I',w)),257:(4,1,pack('I',h)),258:(3,count,pack('H'*count,*([depth]*count))),259:(3,1,pack('H',1)),262:(3,1,pack('H',{'linear':34892,'cfa':32803,'mono':1}[kind])),271:(2,12,b'Test Camera\0'),272:(2,17,b'Known RAW Sensor\0'),273:(4,1,b'OFFSET'),274:(3,1,pack('H',orientation)),277:(3,1,pack('H',count)),278:(4,1,pack('I',h)),279:(4,1,pack('I',len(data))),284:(3,1,pack('H',1)),339:(3,count,pack('H'*count,*([1 if depth==16 else 3]*count))),50706:(1,4,bytes([1,4,0,0])),50707:(1,4,bytes([1,1,0,0])),50708:(2,17,b'Known RAW Sensor\0'),50713:(3,2,pack('HH',1,1)),50714:(5,count,b''.join(pack('II',black,1) for _ in range(count))),50717:(4,count,pack('I'*count,*([white]*count))),50721:(10,9,b''.join(pack('ii',round(float(v)*10000000),10000000) for v in cam.ravel())),50728:(5,3,pack('IIIIII',1,2,1,1,1,4)),50778:(3,1,pack('H',21))}
    if kind!='linear':tags.update({33421:(3,2,pack('HH',2,2)),33422:(1,4,bytes([0,1,1,2]))})
    if crop:tags.update({50829:(4,4,pack('IIII',2,2,8,10)),50719:(4,2,pack('II',1,1)),50720:(4,2,pack('II',6,4))})
    offset=8+2+12*len(tags)+4;entries=[];extra=bytearray()
    for tag,(typ,n,payload) in sorted(tags.items()):
        if tag==273:continue
        if len(payload)>4:
            entries.append((tag,pack('HHII',tag,typ,n,offset+len(extra))));extra.extend(payload)
            if len(extra)%2:extra.append(0)
        else:entries.append((tag,pack('HHI',tag,typ,n)+payload.ljust(4,b'\0')))
    entries.append((273,pack('HHII',273,4,1,offset+len(extra))))
    encoded=(b'II' if little else b'MM')+pack('HI',42,8)+pack('H',len(tags))+b''.join(v for _,v in sorted(entries))+pack('I',0)+extra+data
    name=f'{kind}-{depth}-o{orientation}'+('-crop' if crop else '')+'.dng';(ROOT/name).write_bytes(encoded)
    normalized=(sensor.astype(np.float64)-black)/(white-black)
    item=dict(file=name,kind=kind,depth=depth,orientation=orientation,crop=crop,width=w,height=h,sensor=normalized.tolist(),source_sha256=hashlib.sha256(encoded).hexdigest(),matrix=cam.tolist(),wb=[2.,1.,4.])
    manifest.append(item)
for depth in [16,32]:
    for o in range(1,9):encode('linear',depth,o,o==6)
    encode('mono',depth,6)
    encode('cfa',depth)
(ROOT/'manifest.json').write_text(json.dumps(manifest,indent=2))
print(f'Encoded {len(manifest)} deterministic DNG sensor files')
