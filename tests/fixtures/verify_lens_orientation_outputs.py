"""Independent NumPy EXIF permutations and native-camera float optical equations."""
from lens_vignetting_math import *
OUT=ROOT/'qa-output/lens-orientation'
m=json.loads((OUT/'cases.json').read_text(encoding='utf-8'));w,h=m['size'];source=np.fromfile(OUT/'source.rgba32.le',dtype='<f4').reshape(h,w,4)
def orient(pixels,value):
    if value==1:return pixels
    if value==2:return pixels[:,::-1]
    if value==3:return pixels[::-1,::-1]
    if value==4:return pixels[::-1]
    if value==5:return pixels.transpose(1,0,2)
    if value==6:return np.rot90(pixels,-1)
    if value==7:return pixels.transpose(1,0,2)[::-1,::-1]
    if value==8:return np.rot90(pixels,1)
maximum=0.;cache={}
for case in m['cases']:
    p=case['profile'].copy();orientation=case['orientation'];p['orientation']=1
    key=(json.dumps(p,sort_keys=True),case['sampling'])
    if key not in cache:cache[key]=transform(correct(source,p),p,case['sampling'])[0]
    expected=orient(cache[key],orientation);cw,ch=case['size'];actual=np.fromfile(OUT/case['file'],dtype='<f4').reshape(ch,cw,4)
    np.testing.assert_allclose(actual,expected,rtol=4e-6,atol=3e-6,err_msg=case['file']);maximum=max(maximum,float(np.max(abs(actual-expected))))
with zipfile.ZipFile(OUT/'oriented.rphoto') as archive:
    record=json.loads(archive.read('manifest.json'));assert record['layers'][0]['adjustments']['lens_profile']['orientation']==6
    assert archive.read('layers/1-source.raw')==(ROOT/'tests/fixtures/raw-metadata/metadata-le.dng').read_bytes()
print(f"Lens orientation: {len(m['cases'])} independent native-camera/NumPy EXIF cases / {len(m['cases'])*w*h} float pixels; eight directions, off-axis center, four distortion/three TCA/two vignetting models, three samplers; max error {maximum:g}; saved RAW source/orientation preserved")
