"""Independent TIFF IFD writer and NumPy bit packing for 2/4-bit gray inputs."""
from pathlib import Path
import hashlib
import itertools
import json
import struct
import zlib
import numpy as np

ROOT=Path(__file__).resolve().parent/'tiff-gray-packed'
ROOT.mkdir(exist_ok=True)
PROFILE=ROOT.parent/'tiff-gray-float'

def encode(bits,profile,big,endian,white,order,compressed,orientation):
    w,h=17,3
    samples=((np.arange(w*h,dtype='u4').reshape(h,w)*7+3)% (1<<bits)).astype('u1')
    logical=((samples[:,:,None]>>np.arange(bits-1,-1,-1,dtype='u1'))&1).reshape(h,w*bits)
    logical=np.pad(logical,((0,0),(0,(-w*bits)%8)))
    packed=np.packbits(logical,axis=1,bitorder='big' if order==1 else 'little').tobytes()
    payload=zlib.compress(packed) if compressed else packed
    xmp=f'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="{orientation}" tiff:ImageWidth="17" tiff:ImageLength="3"><note>저비트 회색</note></rdf:Description></rdf:RDF></x:xmpmeta>'.encode()
    # TIFF types: SHORT=3, LONG=4, LONG8=16, BYTE=1, UNDEFINED=7.
    scalar=lambda value,kind=3:struct.pack(endian+{3:'H',4:'I',16:'Q'}[kind],value)
    entries={256:(3,1,scalar(w)),257:(3,1,scalar(h)),258:(3,1,scalar(bits)),259:(3,1,scalar(8 if compressed else 1)),262:(3,1,scalar(0 if white else 1)),266:(3,1,scalar(order)),273:(16 if big else 4,1,None),274:(3,1,scalar(orientation)),277:(3,1,scalar(1)),278:(4,1,scalar(h,4)),279:(16 if big else 4,1,scalar(len(payload),16 if big else 4)),339:(3,1,scalar(1)),700:(1,len(xmp),xmp)}
    if profile!='none':
        data=(PROFILE/(profile+'.icc')).read_bytes();entries[34675]=(7,len(data),data)
    header=(b'II' if endian=='<' else b'MM')+scalar(43 if big else 42)
    header+=scalar(8)+scalar(0)+scalar(16,16) if big else scalar(8,4)
    countsize,entrysize,inline=(8,20,8) if big else (2,12,4)
    fixed=len(header)+countsize+len(entries)*entrysize+inline
    extra=bytearray();offsets={}
    for tag,(_,_,value) in sorted(entries.items()):
        if value is not None and len(value)>inline:
            extra.extend(bytes(-(fixed+len(extra))%8));offsets[tag]=fixed+len(extra);extra.extend(value)
    extra.extend(bytes(-(fixed+len(extra))%8));stripoffset=fixed+len(extra)
    entries[273]=(16 if big else 4,1,scalar(stripoffset,16 if big else 4))
    ifd=struct.pack(endian+('Q' if big else 'H'),len(entries))
    for tag,(kind,count,value) in sorted(entries.items()):
        ifd+=struct.pack(endian+('HHQ' if big else 'HHI'),tag,kind,count)
        ifd+=struct.pack(endian+('Q' if big else 'I'),offsets[tag]) if len(value)>inline else value+bytes(inline-len(value))
    ifd+=bytes(inline)
    return header+ifd+extra+payload

cases=[]
for index,(bits,profile,big,endian,white,order,compressed) in enumerate(itertools.product([2,4],['none','linear','gamma18','working'],[False,True],['<','>'],[False,True],[1,2],[False,True])):
    name=f'd{bits}-{profile}-big{int(big)}-end{int(endian == ">")}-white{int(white)}-order{order}-zip{int(compressed)}.tiff'
    orientation=[1,6,8][index%3]
    data=encode(bits,profile,big,endian,white,order,compressed,orientation)
    (ROOT/name).write_bytes(data)
    cases.append(dict(file=name,bits=bits,profile=profile,big=big,endian=endian,white=white,order=order,compressed=compressed,orientation=orientation,sha256=hashlib.sha256(data).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independent packed gray TIFF inputs/{len(cases)*51} pixels')
