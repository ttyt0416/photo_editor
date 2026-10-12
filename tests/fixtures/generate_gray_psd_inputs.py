"""Independent synthetic grayscale PSD/PSB fixtures (Adobe binary layout).
Own struct/zlib encoder; psd-tools independently verifies the encoded channels.
No Photoshop-generated or color-managed fixture claim.
"""
from pathlib import Path
import json, struct, zlib, hashlib
import numpy as np
from psd_tools import PSDImage
ROOT = Path(__file__).resolve().parent / "gray-psd"
W, H = 4, 3

def pack(fmt, *v): return struct.pack(">" + fmt, *v)
def section(data, large=False): return pack("Q" if large else "I", len(data)) + data

def raw(values, depth):
    return pack({8:"B",16:"H",32:"f"}[depth] * len(values), *values)

def encode(values, width, height, depth, large, mode):
    data = raw(values, depth)
    rows = [data[y*width*depth//8:(y+1)*width*depth//8] for y in range(height)]
    if mode == 0: payload = data
    elif mode == 1:
        # Literal-only valid PackBits rows; independent from Rust's run encoder.
        encoded = [bytes([len(row)-1]) + row for row in rows]
        payload = b"".join(pack("I" if large else "H",len(row)) for row in encoded) + b"".join(encoded)
    else:
        if mode == 3:
            predicted=[]
            for row in rows:
                if depth == 16:
                    v=struct.unpack(">"+"H"*width,row)
                    predicted.append(raw([(v[i]-(v[i-1] if i else 0)) & 65535 for i in range(width)],16))
                else:
                    if depth == 32: row=bytes(row[i*4+b] for b in range(4) for i in range(width))
                    predicted.append(bytes((row[i]-(row[i-1] if i else 0)) & 255 for i in range(len(row))))
            data=b"".join(predicted)
        payload=zlib.compress(data)
    return pack("H",mode)+payload

def block(key,data): return b"8BIM"+key+section(data)+(b"\0" if len(data)%2 else b"")
def resource(identifier,data): return b"8BIM"+pack("H",identifier)+b"\0\0"+section(data)+(b"\0" if len(data)%2 else b"")

def record(name,identifier,section_type,values,alpha,mask,depth,large,mode,clipped=False):
    group = section_type != 0
    entries=[]
    if not group: entries=[(0,encode(values,W,H,depth,large,mode)),(-1,encode(alpha,W,H,depth,large,mode))]
    if mask is not None and section_type != 3: entries.append((-2,encode(mask,W,H,depth,large,mode)))
    bounds=[0,0,0,0] if group else [0,0,H,W]
    maskdata=pack("iiiiBBH",0,0,H,W,255,0,0) if mask is not None and section_type!=3 else b""
    utf=name.encode("utf-16be")
    extra=section(maskdata)+section(b"")+b"\0"*4+block(b"luni",pack("I",len(utf)//2)+utf)
    if group: extra+=block(b"lsct",pack("I",section_type))
    if section_type!=3: extra+=block(b"lyid",pack("I",identifier))
    meta=pack("iiiiH",*bounds,len(entries))+b"".join(pack("h",cid)+pack("Q" if large else "I",len(data)) for cid,data in entries)
    meta+=b"8BIM"+(b"pass" if group else b"norm")+bytes([255,int(clipped),0,0])+section(extra)
    return meta,b"".join(data for _,data in entries)

def main():
    ROOT.mkdir(exist_ok=True)
    manifest=[]
    for depth in [8,16,32]:
        maximum={8:255,16:65535,32:1}[depth]
        gray=({8:[0,1,127,128,254,255],16:[0,1,32767,32768,65534,65535],32:[-.25,-0.0,.125,.5,1.25,2.125]}[depth])*2
        alpha=({8:[0,64,128,255],16:[0,16384,32768,65535],32:[0,.25,.5,1]}[depth])*3
        fg=list(reversed(gray))
        mask8=[64,192,255,128]*3
        mask=[round(v*maximum/255) if depth!=32 else v/255 for v in mask8]
        groupmask=[round(192*maximum/255) if depth!=32 else 192/255]*12
        # Synthetic ICC header exercises resource retention/profile-space checking only.
        icc=bytearray(132);icc[:4]=pack("I",132);icc[16:20]=b"GRAY";icc[36:40]=b"acsp"
        resources=resource(1039,icc)+resource(1032,pack("IIIIiB",1,576,576,1,40,0))
        for large in [False,True]:
            for mode in range(4):
                for kind in ["layers","flat-alpha","flat-opaque"]:
                    has_alpha=kind!="flat-opaque"
                    if kind=="layers":
                        specs=[("회색 그룹",10,1,[],[],groupmask,False),("앞 회색",11,0,fg,alpha,mask,True),("뒤 회색",12,0,gray,[maximum]*12,None,False),("끝",10,3,[],[],None,False)]
                        records=[record(n,i,t,g,a,m,depth,large,mode,c) for n,i,t,g,a,m,c in reversed(specs)]
                        info=pack("h",-4)+b"".join(r for r,_ in records)+b"".join(d for _,d in records)
                        info+=b"\0"*((-len(info))%(4 if depth>8 else 2))
                        outer=section(info,large)+section(b"") if depth==8 else section(b"",large)+section(b"")+b"8BIM"+(b"Lr16" if depth==16 else b"Lr32")+section(info,large)
                        factor=[a/maximum*m/maximum for a,m in zip(alpha,mask)]
                        merged=[(b/maximum*(1-f)+v/maximum*f) for b,v,f in zip(gray,fg,factor)]
                        merged=[round(min(max(v,0),1)*maximum) if depth!=32 else v for v in merged]
                        mergedalpha=groupmask
                    else:
                        outer=b"";merged=gray;mergedalpha=alpha
                    channels=[merged]+([mergedalpha] if has_alpha else [])
                    header=b"8BPS"+pack("H",2 if large else 1)+b"\0"*6+pack("HIIHH",len(channels),H,W,depth,1)
                    data=header+section(b"")+section(resources)+section(outer,large)+encode(sum(channels,[]),W,H*len(channels),depth,large,mode)
                    name=f"d{depth}-c{mode}-b{int(large)}-{kind}."+("psb" if large else "psd")
                    path=ROOT/name;path.write_bytes(data)
                    psd=PSDImage.open(path)
                    assert int(psd.color_mode)==1 and psd.depth==depth and psd.size==(W,H)
                    expected=np.asarray(channels,dtype="f4").T.reshape(H,W,-1)/maximum
                    np.testing.assert_array_equal(psd.numpy(),expected)
                    if kind=="layers":
                        group=psd[0]
                        assert group.is_group() and group.name=="회색 그룹" and len(group)==2
                        for layer,g,a in [(group[0],gray,[maximum]*12),(group[1],fg,alpha)]:
                            np.testing.assert_array_equal(layer.numpy(),np.asarray([g,a],dtype="f4").T.reshape(H,W,2)/maximum)
                        np.testing.assert_array_equal(group[1].numpy("mask"),np.asarray(mask,dtype="f4").reshape(H,W,1)/maximum)
                    manifest.append(dict(file=name,sha256=hashlib.sha256(data).hexdigest(),depth=depth,large=large,compression=mode,kind=kind,gray=gray,foreground=fg,alpha=alpha,mask8=mask8,merged=merged,merged_alpha=mergedalpha if has_alpha else [maximum]*12))
    (ROOT/"manifest.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
    print(f"{len(manifest)} independently encoded and psd-tools decoded grayscale fixtures")
if __name__=="__main__": main()
