"""Independent NumPy/tifffile float gray corpus, compressed/classic/BigTIFF/endian."""
from pathlib import Path
import hashlib
import itertools
import json
import struct
import numpy as np
import tifffile

ROOT = Path(__file__).resolve().parent/'tiff-gray-float'
ROOT.mkdir(exist_ok=True)
BASE = (ROOT.parents[2]/'data/color/srgb-gray.icc').read_bytes()


def gamma_profile(gamma):
    result = bytearray(BASE)
    for i in range(struct.unpack_from('>I',result,128)[0]):
        pos = 132+i*12
        name,offset,size = struct.unpack_from('>4sII',result,pos)
        if name == b'kTRC':
            payload = b'curv'+bytes(8) if gamma == 1 else b'para'+bytes(8)+struct.pack('>i',round(gamma*65536))
            assert len(payload) <= size
            result[offset:offset+size] = payload+bytes(size-len(payload))
            struct.pack_into('>I',result,pos+8,len(payload))
            return bytes(result)
    raise AssertionError('Missing gray curve')


profiles = {'none':None, 'linear':gamma_profile(1), 'gamma18':gamma_profile(1.8), 'working':BASE, 'invalid':b'not-an-ICC-profile'}
for name,data in profiles.items():
    if data is not None:
        (ROOT/(name+'.icc')).write_bytes(data)
native = np.array([-2,-.25,-0.0,.001,.125,.5,1,2,16,1e8,0.00001,0.02,0.25,0.75,4,8,32]*3,dtype='f4').reshape(3,17)
cases = []
for index,(name,big,endian,white,compressed) in enumerate(itertools.product(profiles,[False,True],['<','>'],[False,True],[False,True])):
    filename = f'{name}-big{int(big)}-end{endian == ">"}-white{int(white)}-zip{int(compressed)}.tiff'
    path = ROOT/filename
    orientation = [1,6,8][index%3]
    xmp = f'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="{orientation}" tiff:ImageWidth="17" tiff:ImageLength="3"><note>회색 HDR</note></rdf:Description></rdf:RDF></x:xmpmeta>'.encode()
    extratags = [(274,'H',1,orientation,False),(700,'B',len(xmp),xmp,False)]
    if profiles[name] is not None:
        data = profiles[name]
        extratags.append((34675,'B',len(data),data,False))
    kwargs = dict(photometric='miniswhite' if white else 'minisblack',compression='deflate' if compressed else None,rowsperstrip=1,metadata=None,extratags=extratags)
    stored = (np.float32(1)-native) if white else native
    multipage = index in [0,27,79]
    with tifffile.TiffWriter(path,bigtiff=big,byteorder=endian) as writer:
        writer.write(stored,**kwargs)
        if multipage:
            writer.write(np.zeros((1,2),dtype='f4'),photometric='minisblack',metadata=None)
    cases.append(dict(file=filename,profile=name,big=big,endian=endian,white=white,compressed=compressed,orientation=orientation,multipage=multipage,sha256=hashlib.sha256(path.read_bytes()).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independently encoded gray float TIFF files/{len(cases)*51} pixels')
