"""Independent flat RGB native-white/earlier-straight PSD/PSB fixtures."""
from pathlib import Path
import hashlib,json
import numpy as np
from psd_tools import PSDImage
from psd_tools.constants import Compression
from generate_gray_psd_inputs import encode,section,pack
ROOT=Path(__file__).resolve().parent/"psd-matte"
def main():
    ROOT.mkdir(exist_ok=True);manifest=[]
    for depth in [8,16,32]:
        maximum={8:255,16:65535,32:1}[depth]
        r=([0,1,maximum//2,maximum-1,maximum,maximum//4] if depth!=32 else [-.25,-0.0,.125,.5,1.25,2.125])*2
        g=list(reversed(r));b=[round(maximum/3) if depth!=32 else .3456789]*12
        a=([0,1,maximum//8,maximum//2,maximum-1,maximum] if depth!=32 else [0,1/65535,.125,.5,1,1/255])*2
        source=np.asarray([r,g,b,a],dtype="f4").T/maximum
        for large in [False,True]:
            for mode in range(4):
                for alpha in [False,True]:
                    expected=source.copy()
                    if not alpha:expected[:,3]=1
                    target=expected.astype("f8");color=target[:,:3].copy();weight=target[:,3:4]
                    target[:,:3]=np.where(weight==1,color,color*weight+(1-weight))
                    planes=target.T.astype("f4").tolist() if depth==32 else np.rint(target.T*maximum).astype("u2" if depth==16 else "u1").tolist()
                    if not alpha:planes=planes[:3]
                    header=b"8BPS"+pack("H",2 if large else 1)+b"\0"*6+pack("HIIHH",len(planes),2,6,depth,3)+section(b"")+section(b"")+section(b"",large)
                    name=f"d{depth}-c{mode}-b{int(large)}-a{int(alpha)}."+("psb" if large else "psd")
                    data=header+encode(sum(planes,[]),6,2*len(planes),depth,large,mode);(ROOT/name).write_bytes(data)
                    psd=PSDImage.open(ROOT/name)
                    decoded=psd._record.image_data.get_data(psd._record.header,False)
                    psd._record.image_data.compression=Compression.RAW;psd._record.image_data.data=bytearray(decoded)
                    actual=psd.numpy().reshape(12,-1)
                    actual_weight=actual[:,3:4] if alpha else np.ones((12,1),dtype="f4")
                    np.testing.assert_allclose(actual[:,:3]*actual_weight,expected[:,:3]*expected[:,3:4],rtol=0,atol=4e-6 if depth==32 else 3/maximum+1e-6)
                    legacy_name=None;legacy_sha=None
                    if alpha:
                        legacy_name="legacy-"+name
                        legacy=header+encode(r+g+b+a,6,8,depth,large,mode);(ROOT/legacy_name).write_bytes(legacy);legacy_sha=hashlib.sha256(legacy).hexdigest()
                    manifest.append(dict(file=name,sha256=hashlib.sha256(data).hexdigest(),legacy_file=legacy_name,legacy_sha256=legacy_sha,depth=depth,compression=mode,large=large,alpha=alpha,source=expected.tolist()))
    (ROOT/"manifest.json").write_text(json.dumps(manifest,indent=2)+"\n",encoding="utf-8")
    print("48 independent flat native-white/opaque + 24 earlier-straight inputs; PSD display checked at tiny alpha/HDR/negative values")
if __name__=="__main__":main()
