"""Separate TIFF/EXIF IFD and unreduced rational parser; original RAW project assets."""
from pathlib import Path
import struct,json,hashlib,zipfile
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw-metadata';OUT=ROOT/'qa-output/raw-metadata'
SIZES={1:1,2:1,3:2,4:4,5:8,10:8}
def parse(path):
    data=path.read_bytes();end='<' if data[:2]==b'II' else '>'
    def ifd(at):
        count=struct.unpack_from(end+'H',data,at)[0];result={}
        for i in range(count):
            tag,kind,n=struct.unpack_from(end+'HHI',data,at+2+12*i);value_at=at+2+12*i+8;length=SIZES[kind]*n
            if length>4:value_at=struct.unpack_from(end+'I',data,value_at)[0]
            value=data[value_at:value_at+length]
            if kind==2:result[tag]=value.rstrip(b'\0').decode()
            elif kind==5:result[tag]=[list(struct.unpack_from(end+'II',value,8*j)) for j in range(n)]
            elif kind in [3,4]:result[tag]=list(struct.unpack(end+('H' if kind==3 else 'I')*n,value))
            else:result[tag]=value
        return result
    main=ifd(struct.unpack_from(end+'I',data,4)[0]);exif=ifd(main[34665][0]) if 34665 in main else {};offset=main[273][0];length=main[279][0]
    return data,main,exif,data[offset:offset+length]
records={r['file']:r['shooting'] for r in json.loads((OUT/'records.json').read_text())};manifest=json.loads((FIX/'manifest.json').read_text())
for case in manifest:
    data,main,exif,sensor=parse(FIX/case['file']);assert hashlib.sha256(data).hexdigest()==case['sha256'];assert hashlib.sha256(sensor).hexdigest()==case['sensor_sha256'];_,_,_,base_sensor=parse(ROOT/'tests/fixtures/raw'/case['base']);assert sensor==base_sensor
    def name(tag):
        value=exif.get(tag,main.get(tag));return value.strip() if value and len(value.encode())<=4096 else None
    def rational(tag):
        value=exif.get(tag);return value[0] if value and value[0][0]>0 and value[0][1]>0 else None
    target={'lens_make':name(42035),'lens_model':name(42036),'lens_spec':exif.get(42034),'focal_length':rational(37386),'aperture':rational(33437),'shutter':rational(33434),'iso':exif.get(34867,exif.get(34855,[None]))[0]}
    actual=records[case['file']]
    for key,value in target.items():assert actual[key]==value,(case['file'],key,actual[key],value)
    assert bool(actual['warning'])==case['invalid']
    with zipfile.ZipFile(OUT/f'{case["file"]}.rphoto') as z:
        m=json.loads(z.read('manifest.json'));record=m['layers'][0]['raw_source'];assert record['info']['shooting']==actual;assert z.read('layers/1-source.raw')==data;assert m['version']==9
print('RAW metadata: 4 independent TIFF/EXIF IFDs, two byte orders, lens identity/unaltered rational pairs/ISO32 precedence/invalid fields, identical sensor bytes and original project assets verified')
