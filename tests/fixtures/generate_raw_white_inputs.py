"""Own AsShotWhiteXY DNGs, with sensor bytes copied from our AnalogBalance fixtures."""
from pathlib import Path
import struct,json,hashlib
ROOT=Path(__file__).resolve().parent;OUT=ROOT/'raw-white';OUT.mkdir(exist_ok=True);manifest=[]
for source in json.loads((ROOT/'raw-analog/manifest.json').read_text()):
    data=(ROOT/'raw-analog'/source['file']).read_bytes();end='<' if data[:2]==b'II' else '>';pack=lambda f,*v:struct.pack(end+f,*v);n=struct.unpack_from(end+'H',data,8)[0];original={};sizes={1:1,2:1,3:2,4:4,5:8,10:8}
    for i in range(n):
        p=10+i*12;tag,typ,count=struct.unpack_from(end+'HHI',data,p);size=sizes[typ]*count;start=p+8 if size<=4 else struct.unpack_from(end+'I',data,p+8)[0];original[tag]=(typ,count,data[start:start+size])
    start=struct.unpack(end+'I',original[273][2])[0];length=struct.unpack(end+'I',original[279][2])[0];pixels=data[start:start+length]
    for label,(x,y) in [('A',(44757,40745)),('D65',(31271,32902))]:
        for omit in ([False,True] if source['unit'] else [False]):
            tags=original.copy();del tags[50728];
            if omit:del tags[50727]
            tags[50729]=(5,2,pack('IIII',x,100000,y,100000));offset=8+2+12*len(tags)+4;entries=[];extra=bytearray()
            for tag,(typ,count,payload) in sorted(tags.items()):
                if tag==273:continue
                if len(payload)>4:
                    entries.append((tag,pack('HHII',tag,typ,count,offset+len(extra))));extra.extend(payload)
                    if len(extra)%2:extra.append(0)
                else:entries.append((tag,pack('HHI',tag,typ,count)+payload.ljust(4,b'\0')))
            entries.append((273,pack('HHII',273,4,1,offset+len(extra))));encoded=data[:4]+pack('I',8)+pack('H',len(tags))+b''.join(v for _,v in sorted(entries))+pack('I',0)+extra+pixels;name=Path(source['file']).stem+'-xy'+label+('-noab' if omit else '')+'.dng';(OUT/name).write_bytes(encoded)
            manifest.append({'file':name,'channels':source['channels'],'cfa':source['cfa'],'dual':source['dual'],'unit':source['unit'],'analog':not omit,'xy':[x/100000,y/100000],'source':source['file'],'source_sha256':hashlib.sha256(encoded).hexdigest()})
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2));(OUT/'README.md').write_text('Own54 DNG AsShotWhiteXY fixtures: A/D65 xy, non-unity/identity/absent AnalogBalance, 3/4-channel single/dual profiles, linear16/32/CMYG CFA, two byte orders/crop/orientation. Hardware sensor bytes come from our raw-analog sources unchanged; camera-neutral tags are replaced with known xy rationals. Real-camera/CC/Forward/Reduction/ICC coverage remains separate.\n');print(f'Encoded {len(manifest)} own AsShotWhiteXY DNGs')
