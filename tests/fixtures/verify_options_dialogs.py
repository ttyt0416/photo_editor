"""Independent saved guide layout and unchanged source PNG inspection."""
from pathlib import Path
from zipfile import ZipFile
from PIL import Image
import json
import io
root=Path(__file__).resolve().parents[2]
with ZipFile(root/'qa-output/options-dialogs/guides.rphoto') as archive:
    assert archive.testzip() is None
    m=json.loads(archive.read('manifest.json'))
    assert m['format']=='photo-editor.project' and m['version']==8
    assert (m['width'],m['height'])==(256,256) and len(m['layers'])==1
    layout=m['layout']; guides=layout['guides']
    assert len(guides)==4096
    assert guides[:4095]==[{'axis':i%2,'position':float(i%256)} for i in range(4095)]
    assert guides[-1]=={'axis':0,'position':128.0}
    assert layout['spacing']==[100.0,100.0] and layout['subdivisions']==4
    assert layout['guides_visible'] and not layout['rulers'] and not layout['grid_visible'] and not layout['snap']
    source=Image.open(io.BytesIO(archive.read('layers/1.png'))).convert('RGBA')
    assert source.size==(256,256) and source.getextrema()==((255,255),)*4
print('Option dialogs: independent v8 ZIP/CRC, exact 4095 original guides + one added guide at capacity4096, default grid/snap and unchanged 65536 source pixels')
