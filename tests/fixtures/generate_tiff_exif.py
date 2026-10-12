"""Independent classic TIFF images with camera/EXIF/GPS/interop IFD graph."""
from pathlib import Path
import hashlib, io, json, struct, itertools
import numpy as np
import tifffile
ROOT = Path(__file__).resolve().parent/'tiff-exif'
ROOT.mkdir(exist_ok=True)
cases=[]
for mode,endian,orientation,big in itertools.product(['rgb8','planar16','gray32','palette'],['<','>'],[1,6,8],[False,True]):
    stream=io.BytesIO()
    pixels=np.arange(45,dtype='u1').reshape(3,5,3)*5
    options=dict(byteorder=endian,metadata=None,bigtiff=big)
    cs,es,inline=(8,20,8) if big else (2,12,4)
    countfmt,offsetfmt,entryfmt=("Q","Q","HHQ") if big else ("H","I","HHI")
    if mode=='rgb8': tifffile.imwrite(stream,pixels,photometric='rgb',**options)
    elif mode=='planar16': tifffile.imwrite(stream,np.moveaxis(pixels.astype('u2')*257,2,0),photometric='rgb',planarconfig='separate',**options)
    elif mode=='gray32': tifffile.imwrite(stream,np.arange(15,dtype='f4').reshape(3,5)/7,photometric='minisblack',**options)
    else: tifffile.imwrite(stream,np.arange(15,dtype='u1').reshape(3,5),photometric='palette',colormap=np.stack([np.arange(256)*257]*3).astype('u2'),**options)
    source=bytearray(stream.getvalue())
    root=struct.unpack_from(endian+offsetfmt,source,8 if big else 4)[0]
    n=struct.unpack_from(endian+countfmt,source,root)[0]
    original=[bytes(source[root+cs+i*es:root+cs+(i+1)*es]) for i in range(n)]
    original=[e for e in original if struct.unpack_from(endian+'H',e)[0] not in [274,271,272,34665,34853]]
    source.extend(bytes(4096-len(source)))
    entries={}
    def payload(kind,values):
        if isinstance(values,bytes): return values,len(values)
        scalar={3:'H',4:'I',5:'I',16:'Q'}[kind]
        data=struct.pack(endian+scalar*len(values),*values)
        return data, len(values)//2 if kind==5 else len(values)
    def make_ifd(fields):
        source.extend(bytes(len(source)%2))
        at=len(source);source.extend(bytes(cs+es*len(fields)+inline))
        struct.pack_into(endian+countfmt,source,at,len(fields))
        for i,(tag,kind,values) in enumerate(sorted(fields)):
            data,count=payload(kind,values)
            entry=at+cs+i*es
            struct.pack_into(endian+entryfmt,source,entry,tag,kind,count)
            if len(data)>inline:
                source.extend(bytes(len(source)%2));offset=len(source);source.extend(data)
                struct.pack_into(endian+offsetfmt,source,entry+4+inline,offset)
            else:
                source[entry+4+inline:entry+4+inline+len(data)]=data
        return at
    interop=make_ifd([(1,2,b'R98\0'),(2,7,b'0100')])
    exif=make_ifd([(33434,5,[1,125]),(33437,5,[28,10]),(34855,3,[400]),(36864,7,b'0232'),
                   (36867,2,b'2026:10:12 14:30:00\0'),(37500,7,b'MakerQA opaque data\0'),
                   (37510,7,b'UNICODE\0'+bytes([254,255])+'촬영 테스트'.encode('utf-16-be')),
                   (40962,4,[5]),(40963,4,[3]),(40965,16 if big else 4,[interop]),(42036,2,b'LensQA 35mm\0')])
    gps=make_ifd([(0,1,bytes([2,3,0,0])),(1,2,b'N\0'),(2,5,[37,1,33,1,0,1]),(3,2,b'E\0'),(4,5,[126,1,58,1,0,1])])
    camera=[(271,2,b'CameraQA\0'),(272,2,b'ModelQA\0'),(274,3,[orientation]),(315,2,b'PhotographerQA\0'),(34665,16 if big else 4,[exif]),(34853,16 if big else 4,[gps])]
    newroot=make_ifd(camera)
    count=struct.unpack_from(endian+countfmt,source,newroot)[0]
    raw=[bytes(source[newroot+cs+i*es:newroot+cs+(i+1)*es]) for i in range(count)]
    # Build a final root without moving original image payload offsets.
    final=len(source);all_entries=sorted(original+raw,key=lambda e:struct.unpack_from(endian+'H',e)[0])
    source.extend(struct.pack(endian+countfmt,len(all_entries))+b''.join(all_entries)+bytes(inline))
    struct.pack_into(endian+offsetfmt,source,8 if big else 4,final)
    name=f'{mode}-b{int(big)}-e{int(endian==chr(62))}-o{orientation}.tiff'
    (ROOT/name).write_bytes(source)
    cases.append(dict(file=name,mode=mode,big=big,endian=endian,orientation=orientation,root=final,exif=exif,gps=gps,interop=interop,sha256=hashlib.sha256(source).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independent classic/BigTIFF camera/EXIF/GPS/interop inputs')
