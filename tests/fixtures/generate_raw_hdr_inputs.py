"""Own finite-HDR DNG variants of the existing independent sensor fixtures."""
from pathlib import Path
import struct,json,hashlib
import numpy as np
ROOT=Path(__file__).resolve().parent;OUT=ROOT/'raw-hdr';OUT.mkdir(exist_ok=True)
cases=[]
for template in ['linear-32-o1.dng','linear-32-o6-crop.dng','mono-32-o6.dng']:
    data=bytearray((ROOT/'raw'/template).read_bytes());end='<' if data[:2]==b'II' else '>';offset=struct.unpack_from(end+'I',data,4)[0];count=struct.unpack_from(end+'H',data,offset)[0];entries={}
    for i in range(count):
        position=offset+2+i*12;tag,typ,n,value=struct.unpack_from(end+'HHII',data,position);entries[tag]=(typ,n,value,position)
    start=entries[273][2];length=entries[279][2];mono=template.startswith('mono');maximum=float(np.finfo('f4').max)
    if mono:values=np.resize(np.array([maximum,maximum/2,1e-38,.25,0],dtype='f4'),length//4)
    else:
        sample=np.array([[maximum,maximum/2,maximum/4],[maximum/4,maximum,maximum/2],[maximum/2,maximum/4,maximum],[1e-38,1e-36,1e-30],[float(np.nextafter(np.float32(0),np.float32(1)))*1024,float(np.finfo('f4').tiny),0],[.1,.2,.3],[0,0,0]],dtype='f4')
        values=np.resize(sample,(length//12,3)).reshape(-1)
        # XYZ camera matrix identity exposes signed/out-of-gamut conversion values too.
        typ,n,at,_=entries[50721];assert typ==10 and n==9
        data[at:at+72]=b''.join(struct.pack(end+'ii',int(x),1) for x in np.eye(3).ravel())
    data[start:start+length]=values.astype(end+'f4').tobytes();name=template.replace('.dng','-hdr.dng');(OUT/name).write_bytes(data)
    cases.append({'file':name,'template':template,'source_sha256':hashlib.sha256(data).hexdigest(),'sample_min':float(values.min()),'sample_max':float(values.max())})
(OUT/'manifest.json').write_text(json.dumps(cases,indent=2));(OUT/'README.md').write_text('Own finite float32 HDR sensor DNG variants. Encoded by generate_raw_hdr_inputs.py from our original fixtures; source sensor values, camera matrix and byte order are independently parsed. Linear RGB/mono, crop and orientation, tiny/zero/maximum finite samples. Not a real camera or CFA fidelity claim.\n')
print(f'Encoded {len(cases)} own finite HDR sensor DNG variants')
