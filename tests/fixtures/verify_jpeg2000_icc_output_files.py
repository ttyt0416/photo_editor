"""Independent C JPEG2000 samples, profile/alpha metadata and neutral gray ICC."""
from pathlib import Path
from PIL import Image,ImageCms
import json,struct,io,hashlib,zipfile
import numpy as np
from jpeg2000_c_reference import components,version
ROOT=Path(__file__).resolve().parents[2]/'qa-output/jpeg2000-output/icc'
cases=json.loads((ROOT/'manifest.json').read_text());assert len(cases)==32
profiles={key:(ROOT/(key+'.icc')).read_bytes() for key in ['rgb','gray']}
for key,profile in profiles.items():
    assert profile[16:20]==(b'GRAY' if key=='gray' else b'RGB ')
    assert struct.unpack_from('>iii',profile,68)==(0xf6d6,0x10000,0xd32d)
    for i in range(struct.unpack_from('>I',profile,128)[0]):
        o,n=struct.unpack_from('>II',profile,136+i*12);assert o%4==0 and o+n<=len(profile)
transform=ImageCms.buildTransformFromOpenProfiles(ImageCms.ImageCmsProfile(io.BytesIO(profiles['gray'])),ImageCms.ImageCmsProfile(io.BytesIO(profiles['rgb'])),'L','RGB',renderingIntent=1,flags=ImageCms.Flags.NOOPTIMIZE)
a=Image.fromarray(np.arange(256,dtype='u1').reshape(1,256));b=np.asarray(ImageCms.applyTransform(a,transform));np.testing.assert_array_equal(b,np.repeat(np.arange(256,dtype='u1')[None,:,None],3,axis=2))
def codestream(data):
    at=data.index(b'jp2c');length=struct.unpack_from('>I',data,at-4)[0];return data[at+4:at-4+length]
for case in cases:
    path=ROOT/case['file'];data=path.read_bytes();planes=components(path).astype('i8');limit=(1<<case['depth'])-1
    count=1 if case['gray'] else 3;assert planes.shape==(9,17,count+int(case['alpha']))
    color=np.clip(planes[:,:,:count],0,limit);color=np.repeat(color,3,axis=2) if case['gray'] else color
    alpha=np.clip(planes[:,:,count],0,limit) if case['alpha'] else np.full((9,17),limit)
    expected=np.concatenate([color,alpha[:,:,None]],axis=2).astype('f4')/np.float32(limit);expected[:,:,:3][alpha==0]=0
    actual=np.fromfile(ROOT/case['decoded'],dtype='<f4').reshape(9,17,4);np.testing.assert_array_equal(actual,expected)
    at=data.index(b'colr');length=struct.unpack_from('>I',data,at-4)[0]
    if case['include']:
        assert data[at+4:at+7]==bytes([2,0,0]);assert data[at+7:at-4+length]==profiles['gray' if case['gray'] else 'rgb']
        other=ROOT/case['file'].replace('-itrue.','-ifalse.');assert other!=path
        np.testing.assert_array_equal(planes,components(other))
        assert codestream(data)==codestream(other.read_bytes()),case['file']
    else:assert data[at+4]==1 and struct.unpack_from('>I',data,at+7)[0]==(17 if case['gray'] else 16)
    if case['alpha']:
        at=data.index(b'cdef')+4;assert struct.unpack_from('>H',data,at)[0]==count+1
        assert struct.unpack_from('>HHH',data,at+2+count*6)==(count,1,0)
    with zipfile.ZipFile(ROOT/(case['file']+'.rphoto')) as z:assert z.testzip() is None
assert (ROOT/'profile-ignored.j2k').read_bytes().startswith(bytes([255,79]))
print(f'JP2 ICC output: C OpenJPEG {version},32 files/4896 pixels; exact RGB/gray ICC/D50/alignment/CDEF, all 16 tagged codestreams and decoded samples identical to untagged for lossless/lossy, exact app float normalization/project CRC; 256 exact LittleCMS gray codes, J2K stays raw.')
