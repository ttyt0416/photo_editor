"""Own DNG AnalogBalance encoder; known hardware gains applied to stored samples and neutral tags."""
from pathlib import Path
import struct,json,hashlib
from fractions import Fraction
import numpy as np
ROOT=Path(__file__).resolve().parent;OUT=ROOT/'raw-analog';OUT.mkdir(exist_ok=True);manifest=[]
for source in [('raw-color','linear-3c-16-ill17.dng'),('raw-color','linear-4c-16-ill23.dng'),('raw-color','linear-3c-32-ill21.dng'),('raw-color','linear-4c-32-ill21.dng'),('raw-color','cfa-4c-16-ill213451.dng'),('raw-dual','linear-3c-32-ill17-21.dng'),('raw-dual','linear-4c-32-ill21-17.dng'),('raw-dual','linear-4c-16-ill23-22.dng'),('raw-dual','cfa-4c-16-ill17-21.dng')]:
    data=(ROOT/source[0]/source[1]).read_bytes();end='<' if data[:2]==b'II' else '>';pack=lambda f,*v:struct.pack(end+f,*v);n=struct.unpack_from(end+'H',data,8)[0];tags={};sizes={1:1,2:1,3:2,4:4,5:8,10:8}
    for i in range(n):
        p=10+i*12;tag,typ,count=struct.unpack_from(end+'HHI',data,p);size=sizes[typ]*count;start=p+8 if size<=4 else struct.unpack_from(end+'I',data,p+8)[0];tags[tag]=(typ,count,data[start:start+size])
    channels=tags[50721][1]//3;cfa=tags[262][2]==pack('H',32803);start=struct.unpack(end+'I',tags[273][2])[0];length=struct.unpack(end+'I',tags[279][2])[0];pixels=data[start:start+length]
    original_neutral=tags[50728]
    for unit in [False,True]:
        tags[50728]=original_neutral
        ratios=[(1,1)]*channels if unit else [(3,2),(4,5),(5,4),(7,6)][:channels];tags[50727]=(5,channels,b''.join(pack('II',*r) for r in ratios))
        neutral_tag=tags[50728]
        if not unit:
            payload=neutral_tag[2];values=[Fraction(v,1) for v in struct.unpack(end+'H'*channels,payload)] if neutral_tag[0]==3 else [Fraction(*struct.unpack_from(end+'II',payload,i*8)) for i in range(channels)]
            adjusted=[v*Fraction(*r) for v,r in zip(values,ratios)];tags[50728]=(5,channels,b''.join(pack('II',v.numerator,v.denominator) for v in adjusted))
            width=struct.unpack(end+'I',tags[256][2])[0];height=struct.unpack(end+'I',tags[257][2])[0];cpp=1 if cfa else channels;depth=struct.unpack(end+'H',tags[258][2][:2])[0];dtype=end+('u2' if depth==16 else 'f4')
            sensor=np.frombuffer(pixels,dtype=dtype).reshape(height,width,cpp).astype('f8');gains=np.array([n/d for n,d in ratios])
            if cfa:gains=np.array([[gains[(y%2)*2+x%2] for x in range(width)] for y in range(height)])[:,:,None]
            black=64 if depth==16 else 0;scaled=(sensor-black)*gains+black
            if depth==16:scaled=np.rint(scaled)
            stored=scaled.astype(dtype).tobytes()
        else:stored=pixels

        offset=8+2+12*len(tags)+4;entries=[];extra=bytearray()
        for tag,(typ,count,payload) in sorted(tags.items()):
            if tag==273:continue
            if len(payload)>4:
                entries.append((tag,pack('HHII',tag,typ,count,offset+len(extra))));extra.extend(payload)
                if len(extra)%2:extra.append(0)
            else:entries.append((tag,pack('HHI',tag,typ,count)+payload.ljust(4,b'\0')))
        entries.append((273,pack('HHII',273,4,1,offset+len(extra))));encoded=data[:4]+pack('I',8)+pack('H',len(tags))+b''.join(v for _,v in sorted(entries))+pack('I',0)+extra+stored
        name=source[0]+'-'+Path(source[1]).stem+('-unit' if unit else '-analog')+'.dng';(OUT/name).write_bytes(encoded)
        manifest.append({'file':name,'channels':channels,'cfa':cfa,'dual':50722 in tags,'unit':unit,'source':source[0]+'/'+source[1],'source_sha256':hashlib.sha256(encoded).hexdigest()})
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2));(OUT/'README.md').write_text('Own18 color DNG AnalogBalance fixtures: non-unity/identity gains, single/dual CM, three/four camera channels, 16/32-bit linear/CMYG CFA, byte order/crop/orientation. Known hardware gains are applied to our stored samples and camera-neutral tags, with identity controls. AsShotWhiteXY, CC/Forward/Reduction/look profiles and real-camera/ICC coverage are separate goals.\n')
print(f'Encoded {len(manifest)} own AnalogBalance DNGs')
