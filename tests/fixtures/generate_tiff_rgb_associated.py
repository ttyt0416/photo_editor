"""Independent RGB32F TIFF associated alpha corpus and custom matrix/gamma ICC."""
from pathlib import Path
import hashlib
import itertools
import json
import struct
import numpy as np
import tifffile

ROOT=Path(__file__).resolve().parent/'tiff-rgb-associated'
ROOT.mkdir(exist_ok=True)
WORKING=(ROOT.parents[2]/'data/color/srgb.icc').read_bytes()
custom=bytearray(WORKING)
xyz={'rXYZ':[.55,.25,.02],'gXYZ':[.3,.7,.1],'bXYZ':[.11422,.05,.70421]}
for i in range(struct.unpack_from('>I',custom,128)[0]):
    position=132+i*12
    key,offset,size=struct.unpack_from('>4sII',custom,position)
    key=key.decode()
    if key in xyz:
        custom[offset+8:offset+20]=struct.pack('>iii',*[round(v*65536) for v in xyz[key]])
    if key in ['rTRC','gTRC','bTRC']:
        payload=b'para'+bytes(8)+struct.pack('>i',round(2.2*65536))
        assert len(payload)<=size
        custom[offset:offset+size]=payload+bytes(size-len(payload))
        struct.pack_into('>I',custom,position+8,len(payload))
profiles={'none':None,'working':WORKING,'matrix':bytes(custom)}
for name,data in profiles.items():
    if data is not None:(ROOT/(name+'.icc')).write_bytes(data)
rgb=np.empty((3,17,3),dtype='f4')
rgb[:,:,0]=np.array([-2,-.25,-0.,.001,.125,.5,1,2,16,1e8,0.00001,.02,.25,.75,4,8,32]*3,dtype='f4').reshape(3,17)
rgb[:,:,1]=np.arange(51,dtype='f4').reshape(3,17)/8-1
rgb[:,:,2]=np.flip(rgb[:,:,0],axis=1)*np.float32(.5)
alpha=np.array([0.,-0.,.125,.33333334,1.]*11,dtype='f4')[:51].reshape(3,17)
cases=[]
for index,(profile,planar,big,endian,compressed) in enumerate(itertools.product(profiles,[False,True],[False,True],['<','>'],[False,True])):
    filename=f'{profile}-plan{int(planar)}-big{int(big)}-end{int(endian == ">")}-zip{int(compressed)}.tiff'
    orientation=[1,6,8][index%3]
    xmp=f'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="{orientation}" tiff:ImageWidth="17" tiff:ImageLength="3"><note>RGB 알파 HDR</note></rdf:Description></rdf:RDF></x:xmpmeta>'.encode()
    tags=[(274,'H',1,orientation,False),(700,'B',len(xmp),xmp,False)]
    if profiles[profile] is not None:
        data=profiles[profile];tags.append((34675,'B',len(data),data,False))
    stored=np.concatenate([rgb*alpha[:,:,None],alpha[:,:,None]],axis=2)
    if planar:stored=np.moveaxis(stored,2,0)
    path=ROOT/filename
    tifffile.imwrite(path,stored,photometric='rgb',extrasamples='ASSOCALPHA',planarconfig='separate' if planar else 'contig',bigtiff=big,byteorder=endian,compression='deflate' if compressed else None,rowsperstrip=1,metadata=None,extratags=tags)
    cases.append(dict(file=filename,profile=profile,planar=planar,big=big,endian=endian,compressed=compressed,orientation=orientation,sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independently encoded associated RGB float TIFF inputs/{len(cases)*51} pixels')
