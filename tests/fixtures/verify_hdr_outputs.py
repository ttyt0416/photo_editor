"""Independent RGBE/scanline reader for the Rust Radiance export fixtures."""
from pathlib import Path
import math
root = Path(__file__).resolve().parents[2] / "qa-output" / "hdr-output"
def decode(path):
    data = path.read_bytes(); header, body = data.split(b"\n\n", 1)
    dimensions, body = body.split(b"\n", 1)
    words = dimensions.split(); height, width = int(words[1]), int(words[3])
    offset = 0; pixels = []
    for _ in range(height):
        if 8 <= width <= 32768:
            assert body[offset:offset+4] == bytes([2,2,width>>8,width&255]); offset += 4
            channels=[]
            for _ in range(4):
                row=[]
                while len(row)<width:
                    count=body[offset]; offset+=1; assert count
                    if count>128:
                        row.extend([body[offset]]*(count-128)); offset+=1
                    else:
                        row.extend(body[offset:offset+count]); offset+=count
                assert len(row)==width; channels.append(row)
            encoded=zip(*channels)
        else:
            encoded=[body[offset+i:offset+i+4] for i in range(0,width*4,4)]; offset+=width*4
        for r,g,b,e in encoded:
            pixels.append([math.ldexp(c,e-136) if e else 0.0 for c in (r,g,b)])
    assert offset == len(body)
    return width,height,pixels
for width in (3,16):
    w,h,pixels=decode(root/f"width-{width}.hdr"); assert (w,h)==(width,2)
    for y in range(h):
        for x in range(w):
            alpha=0.0 if x==0 else 0.5 if x==1 else 1.0
            source=[4.0+x/8.0,-0.2 if y==0 else 2.5,0.5]
            expected=[max(0.0,c*alpha+1-alpha) for c in source]
            tolerance=max(1.0,*expected)/128.0
            assert all(abs(a-b)<=tolerance for a,b in zip(pixels[y*w+x],expected))
    _,_,maximum=decode(root/f"max-{width}.hdr")
    assert all(math.isfinite(p[0]) and p[0]>1e38 for p in maximum)
    print(f"Radiance width {width}: RGBE pixels, alpha matting, negative clipping and maximum verified")
