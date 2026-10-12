"""Independently inspect JPEG frames/scans/sampling/density and decode with Pillow."""
from pathlib import Path
from PIL import Image
import struct
root=Path(__file__).resolve().parents[2]/'qa-output/jpeg-output'
expected=list(Image.open(root/'matte.png').convert('RGB').get_flattened_data())
factors=[(1,1),(2,1),(2,2),(1,2),(4,1),(4,2),(1,4),(2,4)]
def segments(data):
    assert data[:2]==b'\xff\xd8' and data[-2:]==b'\xff\xd9'
    i=2
    while i<len(data):
        if data[i]!=255:i+=1;continue
        while data[i]==255:i+=1
        code=data[i];i+=1
        if code==0 or 208<=code<=217:continue
        size=struct.unpack_from('>H',data,i)[0]
        assert size>=2 and i+size<=len(data)
        yield code,data[i+2:i+size]
        i+=size
count=0
for path in sorted(root.glob('s*-p*-o*.jpg')):
    sampling=int(path.name.split('-')[0][1:]);progressive=path.name.split('-')[1]=='p1'
    tags=list(segments(path.read_bytes()));code=194 if progressive else 192
    frame=next(payload for tag,payload in tags if tag==code)
    assert struct.unpack_from('>HH',frame,1)==(29,37)
    h,v=factors[sampling-1];assert frame[7]==h*16+v
    assert next(payload for tag,payload in tags if tag==221)==b'\x00\x03'
    if progressive:assert sum(tag==218 for tag,_ in tags)>3
    image=Image.open(path);assert image.size==(37,29) and image.info['jfif_density']==(300,300)
    assert bool(image.info.get('progressive',False))==progressive
    actual=list(image.convert('RGB').get_flattened_data())
    assert sum(abs(a[c]-b[c]) for a,b in zip(actual,expected) for c in range(3))/len(actual)/3<12
    count+=1
assert count==32
plain=sum(p.stat().st_size for p in root.glob('s*-p*-o0.jpg'))
optimized=sum(p.stat().st_size for p in root.glob('s*-p*-o1.jpg'))
assert optimized<plain
for scans in (2,8,64):
    path=root/f'scans{scans}.jpg';image=Image.open(path);image.load()
    assert image.size==(37,29) and image.info['progressive']
print('JPEG: 32 sampling/progressive/optimized outputs, scans 2/8/64, restart markers, 300ppi, matte/pixels and smaller optimized total verified with Pillow')
