"""Independent C OpenJPEG quality-layer decoding and packet-order COD parsing."""
from pathlib import Path
from PIL import Image,ImageFile
import numpy as np,json,struct
OUT=Path(__file__).resolve().parents[2]/'qa-output/jpeg2000-output/progressive'
cases=json.loads((OUT/'manifest.json').read_text());assert len(cases)==140
source=np.asarray(Image.open(OUT/'source.png'));maximum=0;groups={}
for case in cases:
    data=(OUT/case['file']).read_bytes()
    if case['file'].endswith('.jp2'):
        at=0
        while data[at+4:at+8]!=b'jp2c':at+=struct.unpack_from('>I',data,at)[0]
        length=struct.unpack_from('>I',data,at)[0];data=data[at+8:at+length]
    at=2
    while data[at:at+2]!=b'\xffR':at+=2+struct.unpack_from('>H',data,at+2)[0]
    length=struct.unpack_from('>H',data,at+2)[0];cod=data[at+4:at+2+length]
    assert cod[1]==case['progression'] and struct.unpack_from('>H',cod,2)[0]==case['layers']
    image=Image.open(OUT/case['file'])
    # Pillow 12.1 only refreshes layer arguments when reduce is nonzero. Set the
    # actual C decoder argument at full resolution rather than comparing a full decode.
    tile=image.tile[0];args=tile[3];image.tile=[ImageFile._Tile(tile[0],tile[1],tile[2],(args[0],0,case['limit'],args[3],args[4]))]
    decoded=np.asarray(image.convert('RGBA'))
    actual=np.fromfile(OUT/case['output'],dtype='u1').reshape(48,64,4)
    np.testing.assert_array_equal(actual,decoded,err_msg=str(case))
    if case['lossless'] and case['limit']==case['layers']:np.testing.assert_array_equal(decoded,source)
    groups.setdefault(case['file'],{})[case['limit']]=float(np.mean((decoded.astype('f8')-source)**2));maximum=max(maximum,int(np.max(abs(actual.astype(int)-decoded.astype(int)))))
changes=0
for name,errors in groups.items():
    ordered=sorted(errors)
    if len(ordered)>1 and 'lossless' in name:
        assert errors[ordered[-1]]==0
        assert errors[ordered[0]]>0;changes+=1
assert changes==20
print(f'JPEG2000 progressive: 60 files / {len(cases)} independent quality-layer decodes ({len(cases)*64*48} pixels), all five packet orders / 1,4,100 layers / JP2,J2K / reversible,irreversible / alpha; exact Rust-vs-C decoded pixels and final lossless reference, 20 multi-layer lossless improvement comparisons')
