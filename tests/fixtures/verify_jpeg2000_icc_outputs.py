"""C component decoding, LittleCMS/ICC matrix math and exact JP2 output checks."""
from pathlib import Path
import os, json, io, struct, zipfile
os.environ['OPENCV_LOG_LEVEL']='SILENT'
from PIL import Image, ImageCms
import numpy as np, cv2
from jpeg2000_c_reference import components, version
from icc_reference import tags, curve, matrix, encode_srgb
ROOT=Path(__file__).resolve().parents[2]/'qa-output/jpeg2000-input/icc'
cases=json.loads((ROOT/'manifest.json').read_text());assert len(cases)==60
destination=matrix(tags((ROOT/'srgb.icc').read_bytes()))
max_eight=0;max_math=0
for case in cases:
    path=ROOT/case['file'];planes=components(path).astype('i8');target=(1<<case['depth'])-1
    precision=case['precision'];limit=(1<<precision)-1;gray=case['gray'];count=1 if gray else 3
    rgb=planes[:,:,:count]*target//limit
    if gray:rgb=np.repeat(rgb,3,axis=2)
    alpha=(np.clip(planes[:,:,count]+32768,0,65535)*target//65535) if case['alpha'] else np.full((5,7),target)
    normalized=np.concatenate([rgb,alpha[:,:,None]],axis=2).astype('f4')/np.float32(target)
    source=np.fromfile(ROOT/(case['file']+'.src.f32'),'<f4').reshape(5,7,4)
    np.testing.assert_array_equal(source,normalized)
    actual=np.fromfile(ROOT/(case['file']+'.dst.f32'),'<f4').reshape(5,7,4)
    profile=(ROOT/(case['profile']+'.icc')).read_bytes();encoded=path.read_bytes();at=encoded.index(b'colr')
    length=struct.unpack_from('>I',encoded,at-4)[0];assert encoded[at+4:at+7]==bytes([2,0,0]);assert encoded[at+7:at-4+length]==profile
    if case['depth']==8:
        mode='L' if gray else 'RGB';im=Image.fromarray((rgb[:,:,0] if gray else rgb).astype('u1'))
        transform=ImageCms.buildTransformFromOpenProfiles(ImageCms.ImageCmsProfile(io.BytesIO(profile)),ImageCms.createProfile('sRGB'),mode,'RGB',renderingIntent=1,flags=ImageCms.Flags.NOOPTIMIZE)
        reference=np.asarray(ImageCms.applyTransform(im,transform));error=np.max(np.abs(np.rint(actual[:,:,:3]*255)-reference));assert error<=1,(case['file'],error);max_eight=max(max_eight,error)
    else:
        t=tags(profile)
        if gray:linear=np.repeat(curve(t['kTRC'],source[:,:,:1].astype('f8')),3,axis=2)
        else:linear=np.stack([curve(t[k],source[:,:,c].astype('f8')) for c,k in enumerate(['rTRC','gTRC','bTRC'])],axis=2)@(np.linalg.inv(destination)@matrix(t)).T
        expected=np.clip(encode_srgb(linear),0,1);error=np.max(np.abs(actual[:,:,:3]-expected));assert error<=.0008,(case['file'],error);max_math=max(max_math,error)
    np.testing.assert_array_equal(np.rint(actual[:,:,3]*target),alpha)
    png=cv2.imdecode(np.frombuffer((ROOT/(case['file']+'.png')).read_bytes(),'u1'),cv2.IMREAD_UNCHANGED)
    expected=np.rint(actual*target).astype('u1' if target==255 else 'u2');expected[:,:,:3][alpha==0]=0
    np.testing.assert_array_equal(png[:,:,[2,1,0,3]],expected)
    with zipfile.ZipFile(ROOT/(case['file']+'.rphoto')) as archive:
        assert archive.testzip() is None;m=json.loads(archive.read('manifest.json'));assert (m['width'],m['height'])==(7,5) and m['layers'][0]['bit_depth']==case['depth']
print(f'JP2 ICC: C OpenJPEG {version},60 inputs/2100 pixels; exact pre-ICC component normalization and alpha, method2 profile bytes; LittleCMS8-bit <=1 unit (max {max_eight:g}), independent ICC16-bit math <=0.0008 (max {max_math:.8g}); exact PNG and project ZIP/CRC/depth.')
