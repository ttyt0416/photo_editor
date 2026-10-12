"""Check WebP RIFF/VP8/VP8L mode and alpha with Pillow's independent decoder."""
from pathlib import Path
from PIL import Image
import struct
root=Path(__file__).resolve().parents[2]/'qa-output/webp-output'
source=Image.open(root/'source.png').convert('RGBA')
expected=list(source.get_flattened_data())
def chunks(data):
    assert data[:4]==b'RIFF' and data[8:12]==b'WEBP'
    assert struct.unpack_from('<I',data,4)[0]+8==len(data)
    index=12
    result=[]
    while index<len(data):
        name=data[index:index+4];size=struct.unpack_from('<I',data,index+4)[0]
        assert index+8+size<=len(data)
        result.append(name)
        index+=8+size+(size%2)
    assert index==len(data)
    return result
for path in [root/'lossless.webp',*sorted(root.glob('q*-m*.webp'))]:
    decoded=Image.open(path).convert('RGBA');assert decoded.size==source.size
    actual=list(decoded.get_flattened_data());names=chunks(path.read_bytes())
    if path.name=='lossless.webp':
        assert b'VP8L' in names and actual==expected
    else:
        assert b'VP8 ' in names and b'ALPH' in names
        assert all(a[3]==b[3] for a,b in zip(actual,expected))
        if path.name.startswith(('q90-','q100-')):
            error=sum(abs(a[c]-b[c])*b[3] for a,b in zip(actual,expected) for c in range(3))
            coverage=sum(3*b[3] for b in expected)
            assert error/coverage<4
assert len(list(root.glob('q*-m*.webp')))==15
print('WebP: 15 quality/effort combinations, lossless pixels, lossy VP8, RIFF sizes, exact alpha and weighted high-quality RGB verified with Pillow')
