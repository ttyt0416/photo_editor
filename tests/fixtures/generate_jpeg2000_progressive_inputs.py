"""Own RGBA images encoded independently by Pillow/C OpenJPEG in all packet orders."""
from pathlib import Path
from PIL import Image,ImageFile,features
import numpy as np,json,hashlib
OUT=Path(__file__).resolve().parent/'jpeg2000-progressive';OUT.mkdir(exist_ok=True)
y,x=np.indices((47,65));pixels=np.stack([(x*17+y*53+11)%256,(x*47+y*31+29)%256,(x*59+y*7+83)%256,np.where(x%5==0,0,np.where(x%5==1,127,255))],axis=2).astype('u1');pixels[pixels[:,:,3]==0,:3]=0
source=Image.fromarray(pixels);source.save(OUT/'source.png');cases=[]
for order in ['LRCP','RLCP','RPCL','PCRL','CPRL']:
    for codestream in [False,True]:
        for lossless in [False,True]:
            name=f'{order}-{lossless}.'+('j2k' if codestream else 'jp2');path=OUT/name
            source.save(path,format='JPEG2000',no_jp2=codestream,quality_mode='rates',quality_layers=[8,3,0] if lossless else [32,16,8],irreversible=not lossless,progression=order,mct=1,num_resolutions=5)
            for limit in [1,2,3]:
                image=Image.open(path);tile=image.tile[0];args=tile[3];image.tile=[ImageFile._Tile(tile[0],tile[1],tile[2],(args[0],0,limit,args[3],args[4]))];decoded=np.asarray(image.convert('RGBA'));output=name+f'-quality{limit}.rgba8';(OUT/output).write_bytes(decoded.tobytes())
                if lossless and limit==3:np.testing.assert_array_equal(decoded,pixels)
                cases.append({'file':name,'limit':limit,'output':output,'sha256':hashlib.sha256(path.read_bytes()).hexdigest()})
(OUT/'manifest.json').write_text(json.dumps(cases,indent=2));(OUT/'README.md').write_text(f'Own 65x47 RGBA samples; independently encoded by Pillow/C OpenJPEG {features.version_codec("jpg_2000")}, all five progression orders, JP2/J2K, reversible/irreversible, three quality layers. Expected full/partial results are C decoded. No external images. Complete ISO/other modes/ICC/large files remain separate.\n')
print('Encoded20 own C OpenJPEG inputs /60 partial-layer expectations')
