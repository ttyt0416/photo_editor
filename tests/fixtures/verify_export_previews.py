"""Verify preview byte counts and displayed pixels with an independent decoder."""
from pathlib import Path
from PIL import Image, UnidentifiedImageError
import json,numpy as np
import gzip, base64, io, xml.etree.ElementTree as ET
root=Path(__file__).resolve().parents[2]/'qa-output/export-preview/formats'
manifest=json.loads((root/'manifest.json').read_text())
assert len(manifest)==19
count=0
for entry in manifest:
 path=root/entry['file'];assert path.stat().st_size==entry['bytes']
 preview=np.asarray(Image.open(root/entry['preview']).convert('RGBA'))
 assert list(preview.shape[1::-1])==entry['size']
 if path.suffix in ('.svg','.svgz'):
  source=gzip.decompress(path.read_bytes()) if path.suffix=='.svgz' else path.read_bytes()
  nodes=[n for n in ET.fromstring(source).iter() if n.tag.rsplit('}',1)[-1]=='image'];assert len(nodes)==1
  value=nodes[0].attrib['href'];header,payload=value.split(',',1);assert header=='data:image/png;base64'
  actual=np.asarray(Image.open(io.BytesIO(base64.b64decode(payload,validate=True))).convert('RGBA'))
  assert np.array_equal(actual[:,:,3],preview[:,:,3]);assert abs(actual[:,:,:3].astype(int)-preview[:,:,:3].astype(int))[actual[:,:,3]>0].max()<=2
  continue
 if path.suffix in ('.exr','.hdr','.ff'):continue
 actual=np.asarray(Image.open(path).convert('RGBA'))
 assert actual.shape==preview.shape
 assert np.array_equal(actual[:,:,3],preview[:,:,3]),path.name
 difference=abs(actual[:,:,:3].astype(int)-preview[:,:,:3].astype(int))
 opaque=actual[:,:,3]>0
 assert difference[opaque].max()<=3,(path.name,difference[opaque].max())
 count+=1
assert count==14
low=np.asarray(Image.open(root/'jpeg-low.png'));high=np.asarray(Image.open(root/'jpeg-high.png'))
assert not np.array_equal(low,high)
print('Export preview: actual byte counts/dimensions for 19 outputs, displayed pixels independently compared for 14 Pillow codecs, JPEG quality changes verified; SVG/SVGZ native PNG/XML/gzip independently checked; EXR/HDR/Farbfeld pixels covered by Rust roundtrip tests')
