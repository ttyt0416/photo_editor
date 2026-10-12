"""Independent TIFF IPTC BYTE/UNDEFINED/LONG and Photoshop resource inputs."""
from pathlib import Path
import hashlib,itertools,json,struct
import numpy as np
import tifffile
ROOT=Path(__file__).resolve().parent/'tiff-iptc';ROOT.mkdir(exist_ok=True)
def dataset(record,tag,value):
    return bytes([0x1c,record,tag])+len(value).to_bytes(2,'big')+value
iptc=b''.join(dataset(*v) for v in [(1,90,b'\x1b%G'),(2,5,b'TitleQA'),(2,25,b'keyword'),(2,25,'촬영'.encode()),(2,80,b'PhotographerQA'),(2,116,'원문 저작권'.encode()),(2,120,'한글 설명\n두 번째 줄'.encode()),(2,200,b'\0\xff\x1c\x80')])
(ROOT/'reference.iim').write_bytes(iptc)
def resource(identifier,name,value):
    pascal=bytes([len(name)])+name;pascal+=bytes(len(pascal)%2)
    return b'8BIM'+struct.pack('>H',identifier)+pascal+len(value).to_bytes(4,'big')+value+bytes(len(value)%2)
photoshop=resource(0x03ee,b'a',b'unknown\0\xff')+resource(0x0404,b'IPTC',iptc)+resource(0x0424,b'',b'<xmp>preserve all resources</xmp>')
(ROOT/'reference.irb').write_bytes(photoshop)
cases=[]
for mode,variant,big,endian in itertools.product(['rgb8','planar16','gray32','palette'],['byte','undefined','long','photoshop','both'],[False,True],['<','>']):
    values=np.arange(45,dtype='u1').reshape(3,5,3)*5
    tags=[];kind={'byte':1,'undefined':7,'long':4,'both':4}.get(variant)
    raw=None
    if kind:
        if kind==4:
            padded=iptc+bytes(-len(iptc)%4)
            words=struct.unpack('>'+'I'*(len(padded)//4),padded)
            raw=struct.pack(endian+'I'*len(words),*words)
            tags.append((33723,'I',len(words),words,False))
        else:
            raw=iptc;tags.append((33723,'B' if kind==1 else 7,len(iptc),iptc,False))
    if variant in ['photoshop','both']:tags.append((34377,'B',len(photoshop),photoshop,False))
    name=f'{mode}-{variant}-b{int(big)}-e{int(endian==chr(62))}.tiff'
    options=dict(metadata=None,bigtiff=big,byteorder=endian,extratags=tags)
    if mode=='rgb8':tifffile.imwrite(ROOT/name,values,photometric='rgb',**options)
    elif mode=='planar16':tifffile.imwrite(ROOT/name,np.moveaxis(values.astype('u2')*257,2,0),photometric='rgb',planarconfig='separate',**options)
    elif mode=='gray32':tifffile.imwrite(ROOT/name,np.arange(15,dtype='f4').reshape(3,5)/7,photometric='minisblack',**options)
    else:tifffile.imwrite(ROOT/name,np.arange(15,dtype='u1').reshape(3,5),photometric='palette',colormap=np.stack([np.arange(256)*257]*3).astype('u2'),**options)
    cases.append(dict(file=name,mode=mode,kind=kind,big=big,endian=endian,photoshop=variant in ['photoshop','both'],raw=None if raw is None else raw.hex(),sha256=hashlib.sha256((ROOT/name).read_bytes()).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independent TIFF IPTC/Photoshop inputs')
