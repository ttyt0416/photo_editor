"""Independent packed palette TIFF IFD writer and full uint16 color maps."""
from pathlib import Path
import hashlib
import itertools
import json
import struct
import zlib
import numpy as np

ROOT = Path(__file__).resolve().parent/'tiff-palette'
ROOT.mkdir(exist_ok=True)

def encode(bits, profile, big, endian, tiled, compressed, orientation):
    width, height = 17, 19
    count = 1 << bits
    indices = ((np.arange(width*height, dtype='u8').reshape(height,width)*1009+3)%count).astype('u2')
    indices[0, :2] = [0, count-1]
    entries = np.arange(count, dtype='u8')
    colors = np.array([(entries*1009+1)%65536, (65535-entries*3307)%65536,
                       ((entries*entries*73)^(entries*8191+123))%65536], dtype='u2')
    payloads = []
    for y in range(0, height, 16 if tiled else 7):
        for x in range(0, width, 16 if tiled else width):
            block = indices[y:y+(16 if tiled else 7), x:x+(16 if tiled else width)]
            if tiled: block = np.pad(block, ((0,16-block.shape[0]),(0,16-block.shape[1])))
            if bits < 8:
                logical = ((block[:,:,None].astype('u8')>>np.arange(bits-1,-1,-1,dtype='u8'))&1).reshape(block.shape[0],-1)
                logical = np.pad(logical, ((0,0),(0,(-logical.shape[1])%8)))
                raw = np.packbits(logical, axis=1, bitorder='big').tobytes()
            else: raw = block.astype('u1' if bits==8 else endian+'u2').tobytes()
            payloads.append(zlib.compress(raw) if compressed else raw)
    scalar = lambda value, kind=3: struct.pack(endian+{3:'H',4:'I',16:'Q'}[kind], value)
    array = lambda values, kind=3: b''.join(scalar(v,kind) for v in values)
    offsetkind = 16 if big else 4
    offsettag, sizetag = (324,325) if tiled else (273,279)
    xmp = f'<palette-tiff orientation="{orientation}">16비트 색상표</palette-tiff>'.encode()
    tags = {256:(4,1,scalar(width,4)),257:(4,1,scalar(height,4)),258:(3,1,scalar(bits)),259:(3,1,scalar(8 if compressed else 1)),
            262:(3,1,scalar(3)),266:(3,1,scalar(1)),274:(3,1,scalar(orientation)),277:(3,1,scalar(1)),
            320:(3,3*count,colors.astype(endian+'u2').tobytes()),700:(1,len(xmp),xmp),
            offsettag:(offsetkind,len(payloads),array([0]*len(payloads),offsetkind)),
            sizetag:(offsetkind,len(payloads),array([len(p) for p in payloads],offsetkind))}
    if tiled: tags.update({322:(4,1,scalar(16,4)),323:(4,1,scalar(16,4))})
    else: tags[278] = (4,1,scalar(7,4))
    if profile != 'none':
        icc = (ROOT.parents[2]/'data/color/srgb.icc').read_bytes() if profile=='working' else (ROOT.parent/'tiff-rgb-associated/matrix.icc').read_bytes()
        tags[34675] = (7,len(icc),icc)
    header = (b'II' if endian=='<' else b'MM')+scalar(43 if big else 42)
    header += scalar(8)+scalar(0)+scalar(16,16) if big else scalar(8,4)
    countsize, entrysize, inline = (8,20,8) if big else (2,12,4)
    fixed = len(header)+countsize+len(tags)*entrysize+inline
    extra = bytearray(); external = {}
    for tag, (_,_,value) in sorted(tags.items()):
        if len(value)>inline:
            extra.extend(bytes(-(fixed+len(extra))%8)); external[tag]=fixed+len(extra); extra.extend(value)
    extra.extend(bytes(-(fixed+len(extra))%8)); offsets=[]
    for payload in payloads:
        offsets.append(fixed+len(extra));extra.extend(payload);extra.extend(bytes(-(fixed+len(extra))%8))
    tags[offsettag]=(offsetkind,len(payloads),array(offsets,offsetkind))
    position=external[offsettag]-fixed;extra[position:position+len(tags[offsettag][2])]=tags[offsettag][2]
    ifd=struct.pack(endian+('Q' if big else 'H'),len(tags))
    for tag,(kind,count,value) in sorted(tags.items()):
        ifd+=struct.pack(endian+('HHQ' if big else 'HHI'),tag,kind,count)
        ifd+=struct.pack(endian+('Q' if big else 'I'),external[tag]) if len(value)>inline else value+bytes(inline-len(value))
    return header+ifd+bytes(inline)+extra

cases=[]
for index,(bits,profile,big,endian,tiled,compressed) in enumerate(itertools.product((1,2,4,8,16),('none','working','matrix'),(False,True),('<','>'),(False,True),(False,True))):
    orientation=(1,6,8)[index%3]
    name=f'd{bits}-{profile}-b{int(big)}-e{int(endian==">")}-t{int(tiled)}-z{int(compressed)}.tiff'
    data=encode(bits,profile,big,endian,tiled,compressed,orientation)
    (ROOT/name).write_bytes(data)
    cases.append(dict(file=name,bits=bits,profile=profile,big=big,endian=endian,tiled=tiled,compressed=compressed,orientation=orientation,sha256=hashlib.sha256(data).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independent palette TIFF inputs/{len(cases)*323} pixels')
