"""Independent Pillow JPEGs with single/multipart Photoshop APP13 resources."""
from pathlib import Path
import hashlib,json
from PIL import Image
import numpy as np
ROOT=Path(__file__).resolve().parent/'jpeg-iptc';ROOT.mkdir(exist_ok=True)
small=(ROOT.parent/'tiff-iptc/reference.irb').read_bytes()
def resource(identifier,name,data):
    pascal=bytes([len(name)])+name;pascal+=bytes(len(pascal)%2)
    return b'8BIM'+identifier.to_bytes(2,'big')+pascal+len(data).to_bytes(4,'big')+data+bytes(len(data)%2)
large=small+resource(0x0fa0,b'big-unknown',np.arange(100005,dtype='u4').astype('u1').tobytes())
for name,data in [('small.irb',small),('large.irb',large),('empty.irb',b'')]: (ROOT/name).write_bytes(data)
def segment(marker,data,fill=False):
    return b'\xff'+(b'\xff\xff' if fill else b'')+bytes([marker])+(len(data)+2).to_bytes(2,'big')+data
cases=[]
for gray in [False,True]:
    for progressive in [False,True]:
        name=f'base-gray{int(gray)}-prog{int(progressive)}.jpg'
        values=np.arange(45,dtype='u1').reshape(3,5,3)*5
        image=Image.fromarray(values[:,:,0] if gray else values)
        exif=Image.Exif();exif[271]='CameraQA';exif[274]=6
        image.save(ROOT/name,quality=91,progressive=progressive,exif=exif)
        original=(ROOT/name).read_bytes()
        for layout in ['single','split-boundary','split-inside','large','other-fill','empty']:
            data=large if layout=='large' else b'' if layout=='empty' else small
            if layout=='split-boundary': chunks=[data[:22],data[22:]]
            elif layout=='split-inside': chunks=[data[:3],data[3:12],data[12:31],data[31:]]
            else:chunks=[data[i:i+60000] for i in range(0,len(data),60000)] or [b'']
            blocks=b''.join(segment(0xed,b'Photoshop 3.0\0'+chunk,layout=='other-fill') for chunk in chunks)
            if layout=='other-fill':blocks=segment(0xed,b'Other APP13 opaque data')+segment(0xfe,b'comment')+blocks
            encoded=original[:2]+blocks+original[2:]
            filename=f'gray{int(gray)}-prog{int(progressive)}-{layout}.jpg';(ROOT/filename).write_bytes(encoded)
            cases.append(dict(file=filename,base=name,gray=gray,progressive=progressive,layout=layout,resource='large.irb' if layout=='large' else 'empty.irb' if layout=='empty' else 'small.irb',segments=len(chunks),sha256=hashlib.sha256(encoded).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independent JPEG Photoshop/IPTC inputs')
