"""Own classic TIFF/DNG + separate EXIF IFD encoder; original sensor bytes unchanged."""
from pathlib import Path
import struct,json,hashlib
ROOT=Path(__file__).resolve().parent;OUT=ROOT/'raw-metadata';OUT.mkdir(exist_ok=True)
SIZES={1:1,2:1,3:2,4:4,5:8,10:8};manifest=[]
def encode(base,name,invalid=False,long_iso=False):
    source=(ROOT/'raw'/base).read_bytes();end='<' if source[:2]==b'II' else '>';pack=lambda fmt,*v:struct.pack(end+fmt,*v)
    u16=lambda at:struct.unpack_from(end+'H',source,at)[0];u32=lambda at:struct.unpack_from(end+'I',source,at)[0]
    root=u32(4);tags={}
    for i in range(u16(root)):
        at=root+2+i*12;tag,typ,n=struct.unpack_from(end+'HHI',source,at);size=SIZES[typ]*n;payload=source[at+8:at+8+size] if size<=4 else source[u32(at+8):u32(at+8)+size];tags[tag]=(typ,n,payload)
    offset=struct.unpack(end+'I',tags[273][2])[0];length=struct.unpack(end+'I',tags[279][2])[0];sensor=source[offset:offset+length]
    def ascii(s):p=s.encode()+b'\0';return (2,len(p),p)
    def rational(n,d):return (5,1,pack('II',n,d))
    exif={42035:ascii('Test Optics'),42036:ascii('X'*5000 if invalid else 'Known Prime 35mm'),37386:rational(350,0 if invalid else 10),33437:rational(28,10),33434:rational(1,125),34855:(3,1,pack('H',400)),42034:(5,4,pack('IIIIIIII',35,1,35,1,14,10,14,10))}
    if long_iso:exif[34867]=(4,1,pack('I',70000))
    tags[34665]=(4,1,b'EXIF');tags[42036]=ascii('Root lens placeholder')
    header_size=8+2+len(tags)*12+4;main_extra=bytearray();main_entries={}
    for tag,(typ,n,payload) in sorted(tags.items()):
        if tag in [273,34665]:continue
        if len(payload)>4:
            main_entries[tag]=pack('HHII',tag,typ,n,header_size+len(main_extra));main_extra.extend(payload)
            if len(main_extra)%2:main_extra.append(0)
        else:main_entries[tag]=pack('HHI',tag,typ,n)+payload.ljust(4,b'\0')
    exif_at=header_size+len(main_extra);exif_size=2+len(exif)*12+4;exif_extra=bytearray();exif_entries=[]
    for tag,(typ,n,payload) in sorted(exif.items()):
        if len(payload)>4:
            exif_entries.append(pack('HHII',tag,typ,n,exif_at+exif_size+len(exif_extra)));exif_extra.extend(payload)
            if len(exif_extra)%2:exif_extra.append(0)
        else:exif_entries.append(pack('HHI',tag,typ,n)+payload.ljust(4,b'\0'))
    exif_body=pack('H',len(exif))+b''.join(exif_entries)+pack('I',0)+exif_extra
    main_entries[34665]=pack('HHII',34665,4,1,exif_at);main_entries[273]=pack('HHII',273,4,1,exif_at+len(exif_body))
    encoded=source[:4]+pack('I',8)+pack('H',len(tags))+b''.join(value for _,value in sorted(main_entries.items()))+pack('I',0)+main_extra+exif_body+sensor
    (OUT/name).write_bytes(encoded)
    expected={'lens_make':'Test Optics','lens_model':None if invalid else 'Known Prime 35mm','lens_spec':[[35,1],[35,1],[14,10],[14,10]],'focal_length':None if invalid else [350,10],'aperture':[28,10],'shutter':[1,125],'iso':70000 if long_iso else 400}
    manifest.append({'file':name,'base':base,'invalid':invalid,'expected':expected,'sha256':hashlib.sha256(encoded).hexdigest(),'sensor_sha256':hashlib.sha256(sensor).hexdigest()})
encode('linear-16-o1.dng','metadata-le.dng')
encode('linear-32-o2.dng','metadata-be.dng')
encode('linear-32-o1.dng','metadata-iso-long.dng',long_iso=True)
encode('linear-32-o1.dng','metadata-invalid.dng',invalid=True)
(OUT/'manifest.json').write_text(json.dumps(manifest,indent=2),encoding='utf-8')
print('Encoded 4 independent RAW EXIF IFD variants, original sensor bytes unchanged')
