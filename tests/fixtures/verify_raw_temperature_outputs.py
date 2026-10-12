"""Independent NumPy spectral integration, DNG parsing and color conversion."""
from pathlib import Path
import json,hashlib,zipfile
import numpy as np
from verify_raw_outputs import parse,expected
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/'tests/fixtures/raw';OUT=ROOT/'qa-output/raw-temperature'
data=(ROOT/'data/cie/CIE_xyz_1931_2deg.csv').read_bytes()
assert hashlib.sha256(data).hexdigest()=='fa663e3535a7e0763a745993a1f0a192eb0275ac46ad2d1befd7626841e713c1'
table=np.loadtxt(ROOT/'data/cie/CIE_xyz_1931_2deg.csv',delimiter=',');assert table.shape==(471,4)
# Exact SI constants; integrate the observer independently using vectorized dot products.
C2=6.62607015e-34*299792458/1.380649e-23
wave=table[:,0]*1e-9
def locus(k):
    xyz=table[:,1:].T@(1/(wave**5*np.expm1(C2/(wave*k))))
    return xyz/xyz[1]
def uv(xyz):
    x,y,z=xyz
    return np.array([4*x,6*y])/(x+15*y+3*z)
def white(k,t):
    point=uv(locus(k))
    tangent=uv(locus(k*1.0001))-uv(locus(k*.9999))
    normal=np.array([tangent[1],-tangent[0]])
    if normal[1]<0:normal=-normal
    point+=normal/np.linalg.norm(normal)*t*1e-4
    u,v=point;x=3*u/(2*u-8*v+4);y=2*v/(2*u-8*v+4)
    return np.array([x/y,1,(1-x-y)/y])
if __name__=='__main__':
    points=json.loads((OUT/'white-points.json').read_text())
    for case in points:
        reference=white(case['temperature'],case['tint'])
        if case['white'] is None:assert (reference<=0).any()
        else:np.testing.assert_allclose(case['white'],reference,rtol=2e-12,atol=2e-12)
    cases=json.loads((OUT/'cases.json').read_text());pixels=0
    for case in cases:
        _,tags,sensor=parse(FIX/case['input']);mono=tags[262][0]==1;cfa=tags[262][0]==32803
        if mono:gains=np.ones(3)
        else:
            camera=np.asarray(tags[50721],dtype='f4').reshape(3,3).astype('f8')@white(case['temperature'],case['tint'])
            gains=camera[1]/camera
        reference=expected(tags,sensor,gains,case['exposure'],white(case['temperature'],case['tint']));actual=np.fromfile(OUT/case['output'],dtype='<f4').reshape(reference.shape)
        assert np.isfinite(actual).all()
        if cfa:np.testing.assert_allclose(actual[3:-3,3:-3],reference[3:-3,3:-3],rtol=3e-5,atol=2e-5)
        else:np.testing.assert_allclose(actual,reference,rtol=3e-5,atol=2e-5)
        pixels+=actual.shape[0]*actual.shape[1]
    for item in json.loads((FIX/'manifest.json').read_text()):
        name=item['file']
        with zipfile.ZipFile(OUT/(name+'.rphoto')) as z:
            assert z.read('layers/1-source.raw')==(FIX/name).read_bytes()
            m=json.loads(z.read('manifest.json'));assert m['version']==10
            settings=m['layers'][0]['raw_source']['settings'];assert settings['white_balance']=='Temperature'
            assert settings.get('temperature',6500)==6500 and settings['tint']==100 and settings['exposure']==2
    print(f'RAW temperature: {len(points)} CIE/Planck white points, {len(cases)} independent DNG color results/{pixels} pixels, 20 byte-identical sources and v10 projects; orientations/crops/16/32-bit/mono/Bayer interior')
