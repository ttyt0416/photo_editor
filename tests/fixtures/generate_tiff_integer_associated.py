"""Independent associated UINT8/16 RGB TIFF inputs, HDR ratios and ICC."""
from pathlib import Path
import hashlib
import itertools
import json
import numpy as np
import tifffile

ROOT=Path(__file__).resolve().parent/'tiff-integer-associated'
ROOT.mkdir(exist_ok=True)
PROFILE=ROOT.parent/'tiff-rgb-associated'
cases=[]
for index,(bits,profile,planar,big,endian,compressed) in enumerate(itertools.product([8,16],['none','working','matrix'],[False,True],[False,True],['<','>'],[False,True])):
    maximum=(1<<bits)-1
    i=np.arange(51,dtype='u8').reshape(3,17)
    alpha=np.array([0,1,maximum//8,maximum//3,maximum]*11,dtype=f'u{bits//8}')[:51].reshape(3,17)
    rgba=np.stack([(i*37)%(maximum+1),(i*71)%(maximum+1),(i*113)%(maximum+1),alpha],axis=2).astype(f'u{bits//8}')
    rgba[alpha==0,:3]=0
    filename=f'd{bits}-{profile}-plan{int(planar)}-big{int(big)}-end{int(endian == ">")}-zip{int(compressed)}.tiff'
    orientation=[1,6,8][index%3]
    xmp=f'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="{orientation}" tiff:ImageWidth="17" tiff:ImageLength="3"><note>정수 연관 알파</note></rdf:Description></rdf:RDF></x:xmpmeta>'.encode()
    tags=[(274,'H',1,orientation,False),(700,'B',len(xmp),xmp,False)]
    if profile!='none':
        data=(PROFILE/(profile+'.icc')).read_bytes();tags.append((34675,'B',len(data),data,False))
    stored=np.moveaxis(rgba,2,0) if planar else rgba
    path=ROOT/filename
    tifffile.imwrite(path,stored,photometric='rgb',extrasamples='ASSOCALPHA',planarconfig='separate' if planar else 'contig',bigtiff=big,byteorder=endian,compression='deflate' if compressed else None,rowsperstrip=1,metadata=None,extratags=tags)
    cases.append(dict(file=filename,bits=bits,profile=profile,planar=planar,big=big,endian=endian,compressed=compressed,orientation=orientation,sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independently encoded UINT8/16 associated RGB TIFF inputs/{len(cases)*51} pixels')
