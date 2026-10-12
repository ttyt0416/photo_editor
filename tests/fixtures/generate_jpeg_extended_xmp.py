"""Independent Adobe ExtendedXMP chunks with MD5 GUID and byte offsets."""
from pathlib import Path
import hashlib,json
ROOT=Path(__file__).resolve().parent/'jpeg-extended-xmp';ROOT.mkdir(exist_ok=True)
BASE=ROOT.parent/'jpeg-iptc'
small=('<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:description>촬영 원문 — 中文 — 여러 줄\n내용</dc:description></rdf:Description></rdf:RDF></x:xmpmeta>').encode()
large=small.replace('내용'.encode(),('내용 '+ '한글 원문/中文/☃ '*9000).encode())
for name,data in [('small.xmp',small),('large.xmp',large)]: (ROOT/name).write_bytes(data)
def segment(marker,data):return b'\xff'+bytes([marker])+(len(data)+2).to_bytes(2,'big')+data
cases=[]
for gray in [0,1]:
    for progressive in [0,1]:
        base=f'base-gray{gray}-prog{progressive}.jpg';original=(BASE/base).read_bytes()
        for layout in ['single','split-utf8','reverse','large','basic-late']:
            packet=large if layout=='large' else small
            guid=hashlib.md5(packet).hexdigest().upper()
            link=f'<n:HasExtendedXMP>{guid}</n:HasExtendedXMP>' if layout=='basic-late' else ''
            attribute='' if link else f' n:HasExtendedXMP="{guid}"'
            basic=(f'<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:n="http://ns.adobe.com/xmp/note/" xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="6" tiff:ImageWidth="5" tiff:ImageLength="3"{attribute}>{link}</rdf:Description></rdf:RDF></x:xmpmeta>').encode()
            if layout=='split-utf8':
                at=packet.index('촬영'.encode())+1;parts=[(0,packet[:at]),(at,packet[at:])]
            elif layout=='single':parts=[(0,packet)]
            else:parts=[(at,packet[at:at+(73 if layout in ['reverse','basic-late'] else 60000)]) for at in range(0,len(packet),73 if layout in ['reverse','basic-late'] else 60000)]
            if layout in ['reverse','basic-late']:parts=list(reversed(parts))
            extensions=b''.join(segment(0xe1,b'http://ns.adobe.com/xmp/extension/\0'+guid.encode()+len(packet).to_bytes(4,'big')+at.to_bytes(4,'big')+data) for at,data in parts)
            basic_segment=segment(0xe1,b'http://ns.adobe.com/xap/1.0/\0'+basic)
            resources=segment(0xed,b'Photoshop 3.0\0'+(BASE/'small.irb').read_bytes())
            markers=(extensions+basic_segment if layout=='basic-late' else basic_segment+extensions)+resources
            name=f'gray{gray}-prog{progressive}-{layout}.jpg';encoded=original[:2]+markers+original[2:];(ROOT/name).write_bytes(encoded)
            (ROOT/(name+'.basic.xmp')).write_bytes(basic)
            cases.append(dict(file=name,base=base,layout=layout,gray=gray,progressive=progressive,packet='large.xmp' if layout=='large' else 'small.xmp',guid=guid,chunks=len(parts),sha256=hashlib.sha256(encoded).hexdigest()))
(ROOT/'manifest.json').write_text(json.dumps(cases,indent=2)+'\n',encoding='utf-8')
print(f'{len(cases)} independent ExtendedXMP JPEG inputs; large packet {len(large)} bytes')
