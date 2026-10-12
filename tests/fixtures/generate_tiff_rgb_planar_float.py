"""Independent classic/BigTIFF writer: strips/tiles and predictors 2/3."""
from pathlib import Path
import hashlib
import itertools
import json
import struct
import zlib
import numpy as np

ROOT = Path(__file__).resolve().parent/'tiff-rgb-planar-float'
ROOT.mkdir(exist_ok=True)

def encode(samples, bits, gray, white, associated, planar, tiled, big, endian, predicted, orientation, profile):
    height, width, channels = samples.shape
    kind = 'f4'
    payloads = []
    chunkheight = 16 if tiled else 7
    for plane in range(channels if planar else 1):
        values = samples[:, :, plane:plane+1] if planar else samples
        for y in range(0, height, chunkheight):
            for x in range(0, width, 16 if tiled else width):
                block = values[y:y+chunkheight, x:x+(16 if tiled else width)]
                if tiled:
                    block = np.pad(block, ((0, 16-block.shape[0]), (0, 16-block.shape[1]), (0, 0)))
                rows, columns, count = block.shape
                if predicted:
                    shuffled = np.frombuffer(block.astype('>'+kind).tobytes(), 'u1').reshape(rows, columns*count, bits//8).transpose(0, 2, 1).copy().reshape(rows, -1)
                    diff = shuffled.copy()
                    diff[:, count:] = shuffled[:, count:] - shuffled[:, :-count]
                    raw = diff.tobytes()
                else:
                    raw = block.astype(endian+kind).tobytes()
                payloads.append(zlib.compress(raw))
    scalar = lambda value, kind=3: struct.pack(endian+{3:'H', 4:'I', 16:'Q'}[kind], value)
    array = lambda values, kind=3: b''.join(scalar(v, kind) for v in values)
    offsetkind = 16 if big else 4
    xmp = f'<layout-tiff orientation="{orientation}">부분 타일과 여러 행</layout-tiff>'.encode()
    icc = (ROOT.parents[2]/'data/color/srgb.icc').read_bytes() if profile=='working' else (ROOT.parent/'tiff-rgb-associated/matrix.icc').read_bytes()
    offsettag, sizetag = (324, 325) if tiled else (273, 279)
    entries = {256:(4,1,scalar(width,4)), 257:(4,1,scalar(height,4)), 258:(3,channels,array([bits]*channels)),
               259:(3,1,scalar(8)), 262:(3,1,scalar(0 if white else 1 if gray else 2)),
               274:(3,1,scalar(orientation)), 277:(3,1,scalar(channels)), 284:(3,1,scalar(2 if planar else 1)),
               339:(3,channels,array([3]*channels)),
               offsettag:(offsetkind,len(payloads),array([0]*len(payloads),offsetkind)),
               sizetag:(offsetkind,len(payloads),array([len(v) for v in payloads],offsetkind)),
               700:(1,len(xmp),xmp)}
    if profile != 'none': entries[34675] = (7,len(icc),icc)
    if tiled:
        entries.update({322:(4,1,scalar(16,4)), 323:(4,1,scalar(16,4))})
    else:
        entries[278] = (4,1,scalar(chunkheight,4))
    if channels in (2, 4): entries[338] = (3,1,scalar(1 if associated else 2))
    if predicted: entries[317] = (3,1,scalar(3))
    header = (b'II' if endian == '<' else b'MM') + scalar(43 if big else 42)
    header += scalar(8)+scalar(0)+scalar(16,16) if big else scalar(8,4)
    countsize, entrysize, inline = (8,20,8) if big else (2,12,4)
    fixed = len(header)+countsize+len(entries)*entrysize+inline
    extra = bytearray()
    external = {}
    for tag, (_, _, value) in sorted(entries.items()):
        if len(value) > inline:
            extra.extend(bytes(-(fixed+len(extra))%8))
            external[tag] = fixed+len(extra)
            extra.extend(value)
    extra.extend(bytes(-(fixed+len(extra))%8))
    offsets = []
    for payload in payloads:
        offsets.append(fixed+len(extra))
        extra.extend(payload)
        extra.extend(bytes(-(fixed+len(extra))%8))
    entries[offsettag] = (offsetkind,len(payloads),array(offsets,offsetkind))
    position = external[offsettag]-fixed
    extra[position:position+len(entries[offsettag][2])] = entries[offsettag][2]
    ifd = struct.pack(endian+('Q' if big else 'H'),len(entries))
    for tag, (kind, count, value) in sorted(entries.items()):
        ifd += struct.pack(endian+('HHQ' if big else 'HHI'),tag,kind,count)
        ifd += struct.pack(endian+('Q' if big else 'I'),external[tag]) if len(value)>inline else value+bytes(inline-len(value))
    return header+ifd+bytes(inline)+extra

cases = []
for index, (structure, profile, tiled, big, endian, predicted) in enumerate(itertools.product(
        ((3,False),(4,False),(4,True)),('none','working','matrix'),(False,True),(False,True),('<','>'),(False,True))):
    channels, associated = structure
    bits, gray, white, planar = 32, False, False, True
    source = np.resize(np.array([-2,-.25,-0.,0.,2**-149,.125,.5,1,2,16,1e8],dtype='f4'),323).reshape(19,17)
    alpha = np.resize(np.array([0.,-0.,.125,.33333334,1.],dtype='f4'),323).reshape(19,17)
    color = np.stack([source,np.roll(source,3,1),np.roll(source,7,1)],axis=2)
    if associated: color = (color.astype('f8')*alpha[:,:,None].astype('f8')).astype('f4')
    samples = np.concatenate([color,alpha[:,:,None]],axis=2) if channels==4 else color
    orientation = (1,6,8)[index%3]
    name = f'c{channels}-a{int(associated)}-{profile}-t{int(tiled)}-b{int(big)}-e{int(endian==">")}-pred{int(predicted)}.tiff'
    data = encode(samples,bits,gray,white,associated,planar,tiled,big,endian,predicted,orientation,profile)
    (ROOT/name).write_bytes(data)
    cases.append(dict(file=name,channels=channels,associated=associated,profile=profile,tiled=tiled,big=big,endian=endian,predicted=predicted,orientation=orientation,sha256=hashlib.sha256(data).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independent float32 RGB planar TIFF inputs/{len(cases)*323} pixels')
