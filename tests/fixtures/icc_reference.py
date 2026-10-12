"""Independent ICC.1:2010 tag encodings, matrix/TRC and sRGB math for QA."""
import struct
import numpy as np

def tags(data):
    count = struct.unpack_from('>I', data, 128)[0]
    out = {}
    for i in range(count):
        key, offset, size = struct.unpack_from('>4sII', data, 132 + i*12)
        out[key.decode()] = data[offset:offset+size]
    return out
def curve(tag, x):
    if tag[:4] == b'curv':
        n = struct.unpack_from('>I', tag, 8)[0]
        if n == 0: return x
        if n == 1: return x**(struct.unpack_from('>H', tag, 12)[0]/256)
        values = np.array(struct.unpack_from('>'+'H'*n, tag, 12))/65535
        return np.interp(x, np.linspace(0,1,n), values)
    assert tag[:4] == b'para'
    kind = struct.unpack_from('>H', tag, 8)[0]
    params = np.array(struct.unpack_from('>'+'i'*[1,3,4,5,7][kind], tag, 12))/65536
    if kind == 0: return x**params[0]
    assert kind == 3
    g,a,b,c,d = params
    return np.where(x>=d, np.maximum(a*x+b,0)**g, c*x)
def matrix(t):
    return np.column_stack([np.array(struct.unpack_from('>iii',t[k],8))/65536 for k in ['rXYZ','gXYZ','bXYZ']])
def encode_srgb(x):
    return np.where(x<=.0031308, 12.92*x, 1.055*np.maximum(x,0)**(1/2.4)-.055)
