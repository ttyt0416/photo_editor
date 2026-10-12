"""Independent tifffile Gray+Alpha float32 inputs, associated and planar."""
from pathlib import Path
import hashlib
import itertools
import json
import numpy as np
import tifffile

ROOT = Path(__file__).resolve().parent/'tiff-gray-alpha'
ROOT.mkdir(exist_ok=True)
BASE = ROOT.parent/'tiff-gray-float'
gray = np.array([-2,-.25,-0.0,.001,.125,.5,1,2,16,1e8,0.00001,0.02,0.25,.75,4,8,32]*3,dtype='f4').reshape(3,17)
alpha = np.array([0.,-0.,.125,.33333334,1.]*11,dtype='f4')[:51].reshape(3,17)
cases = []
for index,(profile,planar,associated,big,endian,compressed) in enumerate(itertools.product(['none','linear','working'],[False,True],[False,True],[False,True],['<','>'],[False,True])):
    filename = f'{profile}-plan{int(planar)}-assoc{int(associated)}-big{int(big)}-end{int(endian == ">")}-zip{int(compressed)}.tiff'
    orientation = [1,6,8][index%3]
    xmp = f'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="{orientation}" tiff:ImageWidth="17" tiff:ImageLength="3"><note>알파 HDR</note></rdf:Description></rdf:RDF></x:xmpmeta>'.encode()
    tags = [(274,'H',1,orientation,False),(700,'B',len(xmp),xmp,False)]
    if profile != 'none':
        data = (BASE/(profile+'.icc')).read_bytes()
        tags.append((34675,'B',len(data),data,False))
    stored = np.stack([gray*alpha if associated else gray,alpha],axis=0 if planar else 2)
    path = ROOT/filename
    tifffile.imwrite(path,stored,photometric='minisblack',extrasamples='ASSOCALPHA' if associated else 'UNASSALPHA',planarconfig='separate' if planar else 'contig',bigtiff=big,byteorder=endian,compression='deflate' if compressed else None,rowsperstrip=1,metadata=None,extratags=tags)
    cases.append(dict(file=filename,profile=profile,planar=planar,associated=associated,big=big,endian=endian,compressed=compressed,orientation=orientation,sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independently encoded gray-alpha float TIFF inputs/{len(cases)*51} pixels')
