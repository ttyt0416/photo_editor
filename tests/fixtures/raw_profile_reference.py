"""Independent NumPy spectral/CCT and Bradford constants shared by QA scripts."""
import numpy as np
from scipy.optimize import minimize_scalar
from verify_raw_temperature_outputs import locus,uv
B=np.array([[.8951,.2664,-.1614],[-.7502,1.7135,.0367],[.0389,-.0685,1.0296]]);D65=np.array([.95047,1,1.08883])
whites={17:[1.0985,1,.35585],21:D65,23:[.96422,1,.82521],22:[.94972,1,1.22638]}
def cct(xyz):
    target=uv(xyz)
    r=minimize_scalar(lambda t:np.sum((uv(locus(np.exp(t)))-target)**2),bounds=(np.log(1000),np.log(50000)),method='bounded',options={'xatol':1e-13})
    assert r.success
    return np.exp(r.x)

def model(tags,channels,gains):
    from scipy.optimize import brentq
    from verify_raw_outputs import SRGB
    a,b=[cct(whites[tags[i][0]]) for i in [50778,50779]];matrices=[np.asarray(tags[tag],dtype='f4').reshape(channels,3).astype('f8') for tag in [50721,50722]]
    gains=np.asarray(gains,dtype='f4').astype('f8');neutral=1/gains
    def matrix(k):
        weight=np.clip((1/k-1/b)/(1/a-1/b),0,1)
        return (weight*matrices[0]+(1-weight)*matrices[1]).astype('f4').astype('f8')
    def point(k):
        xyz=np.linalg.pinv(matrix(k))@neutral;return xyz/xyz[1]
    # Solve self-consistent color temperature as a scalar root, independently of Rust iteration.
    root=brentq(lambda logk:np.log(cct(point(np.exp(logk))))-logk,np.log(1000),np.log(50000),xtol=1e-7)
    k=np.exp(root);native=matrix(k);white=point(k);assert (white>0).all()
    calibrated=(native@np.linalg.inv(B)@np.diag((B@white)/(B@D65))@B).astype('f4').astype('f8')
    rgb2cam=calibrated@SRGB;rgb2cam/=rgb2cam.sum(axis=1,keepdims=True);camera2rgb=np.linalg.pinv(rgb2cam).astype('f4').astype('f8')
    return camera2rgb
