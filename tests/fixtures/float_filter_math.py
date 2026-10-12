"""Independent float64 alpha-weighted Gaussian reference; SciPy supplies convolution."""
import numpy as np
from scipy.ndimage import convolve1d
LIMIT=float(np.finfo('f4').max)

def gaussian(source,sigma):
    sigma=np.float32(sigma);extent=np.float32(sigma-np.float32(.8))/np.float32(.3)
    extent=np.float32(np.float32(extent+np.float32(1))*np.float32(2))+np.float32(1)
    size=int(max(extent,np.float32(3)));size+=1-size%2
    d=np.arange(size,dtype='f8')-size//2;k=np.exp(-.5*(d/float(sigma))**2);k/=k.sum()
    prem=source.copy();prem[:,:,:3]*=prem[:,:,3,None]
    filtered=convolve1d(convolve1d(prem,k,axis=1,mode='nearest'),k,axis=0,mode='nearest')
    result=np.zeros_like(filtered);result[:,:,3]=filtered[:,:,3].clip(0,1)
    np.divide(filtered[:,:,:3],filtered[:,:,3,None],out=result[:,:,:3],where=filtered[:,:,3,None]>0)
    result[:,:,:3]=result[:,:,:3].clip(-LIMIT,LIMIT)
    return result.astype('f4').astype('f8')

