"""Independent AVIF container/bit-depth/matrix and Pillow decode checks."""
from pathlib import Path
from PIL import Image
import struct
root=Path(__file__).resolve().parents[2]/'qa-output/avif-output'
source=Image.open(root/'source.png').convert('RGBA')
expected=list(source.get_flattened_data())
def children(data):
    offset=0
    while offset<len(data):
        size,tag=struct.unpack_from('>I4s',data,offset);head=8
        if size==1:size=struct.unpack_from('>Q',data,offset+8)[0];head=16
        assert size>=head and offset+size<=len(data)
        yield tag,data[offset+head:offset+size]
        offset+=size
    assert offset==len(data)
def descend(data):
    for tag,payload in children(data):
        yield tag,payload
        if tag==b'meta':yield from descend(payload[4:])
        elif tag in (b'iprp',b'ipco'):yield from descend(payload)
count=0
for path in sorted(root.glob('d*-c*-q*.avif')):
    depth=int(path.name.split('-')[0][1:]);mode=int(path.name.split('-')[1][1:]);quality=int(path.stem.split('-')[2][1:])
    properties=list(descend(path.read_bytes()))
    assert any(tag==b'ftyp' and payload[:4]==b'avif' for tag,payload in properties)
    pixel_properties=[payload for tag,payload in properties if tag==b'pixi']
    assert len(pixel_properties)==2
    assert sorted(p[4] for p in pixel_properties)==[1,3]
    for payload in pixel_properties:assert all(value==depth for value in payload[5:5+payload[4]])
    colors=[payload for tag,payload in properties if tag==b'colr' and payload[:4]==b'nclx']
    configs=[payload for tag,payload in properties if tag==b'av1C']
    assert len(configs)==2 and all(len(c)>=4 for c in configs)
    assert all((10 if c[2]&64 else 8)==depth for c in configs)
    if colors:
        assert struct.unpack_from('>HHH',colors[0],4)==(1,13,[6,1,0][mode])
        assert colors[0][10]&128
    else:
        # ravif uses AV1 sequence-header color information for its default BT.601 case.
        assert mode==0
    decoded=Image.open(path).convert('RGBA');assert decoded.size==source.size
    actual=list(decoded.get_flattened_data())
    assert max(abs(a[3]-b[3]) for a,b in zip(actual,expected))<=1
    if quality>=90:
        errors=[abs(a[c]-b[c]) for a,b in zip(actual,expected) if b[3] for c in range(3)]
        assert sum(errors)/len(errors)<7
    count+=1
assert count==18
print('AVIF: 18 quality/depth/matrix outputs, container bit depths, sRGB/full-range matrix tags, independent pixels/alpha and high-quality error verified with Pillow')
