"""Generate RGB order fixtures through psd-tools' independent binary writer.
Uses earlier synthetic gray samples and a separate struct/zlib channel encoder.
"""
from pathlib import Path
import copy,hashlib,json,struct
import numpy as np
from psd_tools import PSDImage
from psd_tools.constants import ColorMode,Compression,ChannelID,BlendMode
from psd_tools.psd.layer_and_mask import ChannelInfo,ChannelData,ChannelDataList
from psd_tools.psd.image_data import ImageData
from generate_gray_psd_inputs import encode,raw
ROOT=Path(__file__).resolve().parent
OUT=ROOT/"rgb-psd-order"

def colors(gray,depth):
    maximum={8:255,16:65535,32:1}[depth]
    green=[maximum-v if depth!=32 else float(np.float32(1)-np.float32(v)) for v in gray]
    blue=[round(maximum/4) if depth!=32 else .25]*12
    return [gray,green,blue]

def matte(planes,depth):
    maximum={8:255,16:65535,32:1}[depth]
    data=(np.asarray(planes,dtype="f4").T/maximum).astype("f8")
    color=data[:,:3].copy();alpha=data[:,3:4]
    data[:,:3]=np.where(alpha==1,color,color*alpha+(1-alpha))
    return data.T.astype("f4").tolist() if depth==32 else np.rint(data.T*maximum).astype("u2" if depth==16 else "u1").tolist()

def main():
    OUT.mkdir(exist_ok=True)
    original=json.loads((ROOT/"gray-psd/manifest.json").read_text(encoding="utf-8"))
    manifest=[]
    for e in [e for e in original if e["kind"]=="layers"]:
        depth,mode,large=e["depth"],e["compression"],e["large"]
        original_psd=PSDImage.open(ROOT/"gray-psd"/e["file"])
        base=original_psd._record
        base.header.color_mode=ColorMode.RGB;base.header.channels=4
        info=base._get_layer_info()
        for record,data in zip(info.layer_records,info.channel_image_data):
            identifier=record.tagged_blocks.get_data(b"lyid")
            divider=record.tagged_blocks.get_data(b"lsct")
            if divider is not None:
                divider.signature=b"8BIM";divider.blend_mode=BlendMode.PASS_THROUGH
                record.blend_mode=BlendMode.NORMAL  # Explicit divider overrides the generic header.
            if identifier not in [11,12]: continue
            gray=e["foreground"] if identifier==11 else e["gray"]
            rgb=colors(gray,depth)
            newdata=[];newinfo=[]
            for c,values in enumerate(rgb):
                payload=encode(values,4,3,depth,large,mode)
                newdata.append(ChannelData(Compression(mode),payload[2:]))
                newinfo.append(ChannelInfo(ChannelID(c),len(payload)))
            for ci,cd in zip(record.channel_info,data):
                if ci.id<0: newinfo.append(copy.deepcopy(ci));newdata.append(copy.deepcopy(cd))
            record.channel_info=newinfo
            data[:]=newdata
        merged=colors(e["merged"],depth)+[e["merged_alpha"]]
        base.image_data=ImageData(Compression(mode))
        base.image_data.set_data([raw(plane,depth) for plane in matte(merged,depth)],base.header)
        icc=bytearray(base.image_resources[1039].data);icc[16:20]=b"RGB ";base.image_resources[1039].data=bytes(icc)
        for groups in [False,True]:
            psd=copy.deepcopy(base);layerinfo=psd._get_layer_info()
            if not groups:
                pairs=[(r,d) for r,d in zip(layerinfo.layer_records,layerinfo.channel_image_data) if r.tagged_blocks.get_data(b"lyid") in [11,12] and not r.tagged_blocks.get_data(b"lsct")]
                psd.image_data.set_data([raw(plane,depth) for plane in matte(colors(e["merged"],depth)+[[{8:255,16:65535,32:1}[depth]]*12],depth)],psd.header)
                layerinfo.layer_records=type(layerinfo.layer_records)([r for r,_ in pairs]);layerinfo.channel_image_data=type(layerinfo.channel_image_data)([d for _,d in pairs]);layerinfo.layer_count=-len(pairs)
            native_name=f"d{depth}-c{mode}-b{int(large)}-g{int(groups)}."+("psb" if large else "psd")
            with (OUT/native_name).open("wb") as f: psd.write(f)
            decoded=PSDImage.open(OUT/native_name)
            assert int(decoded.color_mode)==3
            layers=list(decoded[0]) if groups else list(decoded)
            assert [l.name for l in layers]==["뒤 회색","앞 회색"]
            maximum={8:255,16:65535,32:1}[depth]
            for layer,identifier in zip(layers,[12,11]):
                gray=e["foreground"] if identifier==11 else e["gray"]
                alpha=e["alpha"] if identifier==11 else [maximum]*12
                target=np.asarray(colors(gray,depth)+[alpha],dtype="f4").T.reshape(3,4,4)/maximum
                np.testing.assert_array_equal(layer.numpy(),target)
            legacy=copy.deepcopy(psd);legacy_info=legacy._get_layer_info();legacy_info.layer_records.reverse();legacy_info.channel_image_data.reverse()
            legacy.image_data.set_data([raw(plane,depth) for plane in colors(e["merged"],depth)+[e["merged_alpha"] if groups else [{8:255,16:65535,32:1}[depth]]*12]],legacy.header)
            legacy_name="legacy-"+native_name
            with (OUT/legacy_name).open("wb") as f: legacy.write(f)
            manifest.append(dict(file=native_name,sha256=hashlib.sha256((OUT/native_name).read_bytes()).hexdigest(),legacy_file=legacy_name,legacy_sha256=hashlib.sha256((OUT/legacy_name).read_bytes()).hexdigest(),depth=depth,compression=mode,large=large,groups=groups,background=colors(e["gray"],depth),foreground=colors(e["foreground"],depth),alpha=e["alpha"],mask8=e["mask8"]))
    (OUT/"manifest.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+"\n",encoding="utf-8")
    print(f"{len(manifest)} native + {len(manifest)} earlier-order RGB inputs; independently written and native channels verified")
if __name__=="__main__": main()
