"""Independent XML/base64/original-file/Pillow checks. Does not launch an editor or browser."""
from pathlib import Path
import base64, io, json, struct, xml.etree.ElementTree as ET, zipfile
import numpy as np
from PIL import Image
ROOT = Path(__file__).resolve().parents[2] / 'qa-output/svg-assets'
def resource(value):
    header, payload = value.split(',', 1)
    assert header.endswith(';base64'), header
    return header[5:-7], base64.b64decode(payload, validate=True)
def images(path):
    return [e for e in ET.parse(path).iter() if e.tag.rsplit('}',1)[-1] in ('image','feImage')]
def pixels(blob):
    return np.array(Image.open(io.BytesIO(blob)).convert('RGBA'),dtype=np.int16)
verified=[]
for case in json.loads((ROOT/'한글 원본/native.json').read_text(encoding='utf-8')):
    root=ROOT/'한글 원본'; nodes=images(root/case['file']); assert len(nodes)==1
    node=nodes[0]; mime, preview=resource(node.attrib['href']); _,blob=resource(node.attrib.get('data-rphoto-image-original',node.attrib['href'])); original=(root/case['original']).read_bytes(); assert blob==original
    p=pixels(original)[3,3]; actual=np.array(case['pixel']); assert np.max(np.abs(p-actual))<=2,(case,p,actual)
    legacy=node.attrib.get('{http://www.w3.org/1999/xlink}href'); assert legacy is None or legacy==node.attrib['href']
    verified.append({'case':case['file'],'mime':mime,'original_bytes':len(blob),'pixel_error':int(np.max(np.abs(p-actual)))})
root=ROOT/'other'
for index,case in enumerate(json.loads((root/'codecs.json').read_text(encoding='utf-8'))):
    node=images(root/case['file'])[0]; mime,preview=resource(node.attrib['href']); assert mime=='image/png'
    _,blob=resource(node.attrib['data-rphoto-image-original']); original=Path(case['input']).read_bytes(); assert blob==original
    assert node.attrib['data-rphoto-image-name']==case['name'],node.attrib
    actual=np.array(Image.open(root/f'codec-{index}-actual.png').convert('RGBA'),dtype=np.int16)
    assert np.max(np.abs(pixels(preview)-actual))<=1
    independent=None
    if case['name'].startswith('owned.'):
        ext=case['name'].rsplit('.',1)[-1]
        if ext=='ff':
            assert blob[:8]==b'farbfeld';w,h=struct.unpack('>II',blob[8:16]);p=np.frombuffer(blob[16:],dtype='>u2').reshape(h,w,4).astype(np.float64); independent=np.floor(p/257+0.5).astype(np.int16)
        elif ext=='exr':
            independent=np.tile(np.array([255,0,102,153],dtype=np.int16),(6,8,1))
        elif ext in ('qoi','ppm'):
            # Separate known pixel formula for this locally generated corpus. Pillow lacks
            # QOI and the image encoder's PAM flavor on this host; no codec conformance claim.
            independent=np.array([[[x*27,y*39,119,255] for x in range(8)] for y in range(6)],dtype=np.int16)
        else: independent=pixels(original)
    if independent is not None: assert np.max(np.abs(independent-actual))<=1,(case['name'],independent[0,0],actual[0,0])
    with zipfile.ZipFile(root/f'codec-{index}.rphoto') as project:
        svgs=[project.read(n) for n in project.namelist() if n.endswith('.svg')];assert len(svgs)==1
        original_nodes=[e for e in ET.fromstring(svgs[0]).iter() if 'data-rphoto-image-original' in e.attrib];assert len(original_nodes)==1
        assert resource(original_nodes[0].attrib['data-rphoto-image-original'])[1]==original
    verified.append({'case':case['file'],'original_bytes':len(blob),'independent_pixels':independent is not None})
(ROOT/'independent.json').write_text(json.dumps(verified,ensure_ascii=False,indent=2),encoding='utf-8')
print(f'SVG image assets: {len(verified)} imports; XML/base64/exact originals/projects and Pillow/known pixels passed.')
