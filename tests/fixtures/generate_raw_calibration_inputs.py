"""Own CameraCalibration/signature DNG encoder; unequal independent CC profile pairs."""
from pathlib import Path
import struct,json,hashlib
import numpy as np
ROOT=Path(__file__).resolve().parent;OUT=ROOT/'raw-calibration';OUT.mkdir(exist_ok=True);manifest=[]
sources=[('raw-color','linear-3c-16-ill17.dng'),('raw-color','linear-4c-32-ill21.dng'),('raw-analog','raw-dual-linear-3c-32-ill17-21-analog.dng'),('raw-analog','raw-dual-linear-4c-32-ill21-17-analog.dng'),('raw-white','raw-dual-linear-3c-32-ill17-21-analog-xyD65.dng'),('raw-white','raw-dual-linear-4c-32-ill21-17-unit-xyA.dng'),('raw-white','raw-dual-cfa-4c-16-ill17-21-analog-xyD65.dng'),('raw-dual','linear-4c-16-ill23-22.dng')]
for group,source in sources:
    data=(ROOT/group/source).read_bytes();end='<' if data[:2]==b'II' else '>';pack=lambda f,*v:struct.pack(end+f,*v);n=struct.unpack_from(end+'H',data,8)[0];original={};sizes={1:1,2:1,3:2,4:4,5:8,10:8}
    for i in range(n):
        p=10+i*12;tag,typ,count=struct.unpack_from(end+'HHI',data,p);size=sizes[typ]*count;start=p+8 if size<=4 else struct.unpack_from(end+'I',data,p+8)[0];original[tag]=(typ,count,data[start:start+size])
    channels=original[50721][1]//3;start=struct.unpack(end+'I',original[273][2])[0];length=struct.unpack(end+'I',original[279][2])[0];pixels=data[start:start+length]
    m1=np.array([[1.03,-.02,-.01],[-.01,1.02,-.01],[.02,-.03,1.01]]);m2=np.array([[.97,.01,.02],[.02,.99,-.01],[-.01,.04,.97]])
    if channels==4:
        a=np.eye(4);b=np.eye(4);a[:3,:3]=m1;b[:3,:3]=m2;a[:3,3]=[.01,-.01,.02];b[:3,3]=[-.01,.02,.01]
        for i in range(3):a[i,i]-=a[i,3];b[i,i]-=b[i,3]
        a[3]=[.01,.02,-.01,.98];b[3]=[-.02,.01,.03,.98];m1,m2=a,b
    for variant in ['match','default-signatures','mismatch','identity','missing-second']:
        tags=original.copy()
        for slot,matrix in enumerate([m1,m2]):
            if slot==1 and (50722 not in tags or variant=='missing-second'):continue
            if variant=='identity':matrix=np.eye(channels)
            tags[50723+slot]=(10,channels*channels,b''.join(pack('ii',round(v*10000000),10000000) for v in matrix.ravel()))
        if variant!='default-signatures':
            camera='카메라 보정 A'.encode()+b'\0';profile=('카메라 보정 B' if variant=='mismatch' else '카메라 보정 A').encode()+b'\0';tags[50931]=(1,len(camera),camera);tags[50932]=(2,len(profile),profile)
        offset=8+2+12*len(tags)+4;entries=[];extra=bytearray()
        for tag,(typ,count,payload) in sorted(tags.items()):
            if tag==273:continue
            if len(payload)>4:
                entries.append((tag,pack('HHII',tag,typ,count,offset+len(extra))));extra.extend(payload)
                if len(extra)%2:extra.append(0)
            else:entries.append((tag,pack('HHI',tag,typ,count)+payload.ljust(4,b'\0')))
        entries.append((273,pack('HHII',273,4,1,offset+len(extra))));encoded=data[:4]+pack('I',8)+pack('H',len(tags))+b''.join(v for _,v in sorted(entries))+pack('I',0)+extra+pixels;name=group+'-'+Path(source).stem+'-cc-'+variant+'.dng';(OUT/name).write_bytes(encoded)
        manifest.append({'file':name,'channels':channels,'variant':variant,'cfa':original[262][2]==pack('H',32803),'dual':50722 in original,'xy':50729 in original,'source':group+'/'+source,'source_sha256':hashlib.sha256(encoded).hexdigest()})
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2));(OUT/'README.md').write_text('Own40 CameraCalibration DNGs: signed unequal3x3/4x4 CC, single/dual CM, Unicode UTF-8 matching/default/mismatching signatures, identity/missing-second controls, AnalogBalance/AsShotNeutral/AsShotWhiteXY, sensor bytes unchanged from our own originals. Real cameras/alternate/three profiles/Forward/Reduction/look/ICC remain separate.\n');print(f'Encoded {len(manifest)} own CameraCalibration DNGs')
