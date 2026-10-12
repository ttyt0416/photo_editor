"""Independent LittleCMS (Pillow NOOPTIMIZE) + ICC tag/NumPy matrix/TRC math.
ICC.1:2010 tag encodings and sRGB inverse transfer, no app transform code.
"""
from pathlib import Path
from PIL import Image, ImageCms
import io, json, struct, zipfile
import numpy as np
ROOT = Path(__file__).resolve().parents[2] / 'qa-output/icc-input'
from icc_reference import tags, curve, matrix, encode_srgb
cases=json.loads((ROOT/'manifest.json').read_text());assert len(cases)==30
reference_matrix=matrix(tags((ROOT/'srgb.icc').read_bytes()))
max_eight=0;max_math=0
for case in cases:
    src=np.fromfile(ROOT/(case['id']+'.src.f32'),'<f4').reshape(9,17,4).astype('f8')
    dst=np.fromfile(ROOT/(case['id']+'.dst.f32'),'<f4').reshape(9,17,4).astype('f8')
    profile=(ROOT/(case['profile']+'.icc')).read_bytes();t=tags(profile)
    if case['mode'] < 2:
        mode='L' if case['gray'] else 'RGB'
        image=Image.fromarray(np.rint((src[:,:,0] if case['gray'] else src[:,:,:3])*255).astype('u1'))
        transform=ImageCms.buildTransformFromOpenProfiles(ImageCms.ImageCmsProfile(io.BytesIO(profile)),ImageCms.createProfile('sRGB'),mode,'RGB',renderingIntent=1,flags=ImageCms.Flags.NOOPTIMIZE)
        expected=np.asarray(ImageCms.applyTransform(image,transform)).astype('i4')
        error=np.max(np.abs(np.rint(dst[:,:,:3]*255)-expected));max_eight=max(max_eight,error)
        assert error<=1,(case['id'],error)
    else:
        if case['gray']:
            linear=np.repeat(curve(t['kTRC'],src[:,:,:1]),3,axis=2)
        else:
            values=np.stack([curve(t[k],src[:,:,c]) for c,k in enumerate(['rTRC','gTRC','bTRC'])],axis=2)
            linear=values @ (np.linalg.inv(reference_matrix) @ matrix(t)).T
        expected=encode_srgb(linear)
        if case['depth'] < 32: expected=np.clip(expected,0,1)
        error=np.max(np.abs(dst[:,:,:3]-expected));max_math=max(max_math,error)
        assert error<=0.0008,(case['id'],error)
    if case['depth'] < 32:
        maximum=(1<<case['depth'])-1
        np.testing.assert_array_equal(np.rint(src[:,:,3]*maximum),np.rint(dst[:,:,3]*maximum))
    else:
        np.testing.assert_array_equal(src[:,:,3].astype('f4').view('u4'),dst[:,:,3].astype('f4').view('u4'))
    with zipfile.ZipFile(ROOT/(case['id']+'.rphoto')) as archive:
        assert archive.testzip() is None
        m=json.loads(archive.read('manifest.json'));assert m['width']==17 and m['height']==9
        assert m['layers'][0]['bit_depth']==case['depth']
print(f'ICC input: 30 cases/4590 pixels, 6 locally generated RGB/gray profiles; 12 LittleCMS NOOPTIMIZE 8-bit cases max {max_eight:g} unit; 18 independent ICC tag/matrix/TRC 16/32-bit cases max normalized error {max_math:.8g} <=0.0008; exact alpha and project ZIP/CRC/depth.')
