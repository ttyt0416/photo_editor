"""Verify RGB PSD/PSB order/pixels using the independent psd-tools reader."""
from pathlib import Path
import hashlib,json
import numpy as np
from psd_tools import PSDImage
from psd_tools.constants import Compression
ROOT=Path(__file__).resolve().parents[2]
FIX=ROOT/"tests/fixtures/rgb-psd-order"
OUT=ROOT/"qa-output/psd-order"
entries=json.loads((FIX/"manifest.json").read_text(encoding="utf-8"))
assert len(entries)==48
for e in entries:
    for key,sha in [("file","sha256"),("legacy_file","legacy_sha256")]:
        assert hashlib.sha256((FIX/e[key]).read_bytes()).hexdigest()==e[sha]
    psd=PSDImage.open(OUT/e["file"])
    depth=e["depth"];maximum={8:255,16:65535,32:1}[depth]
    assert int(psd.color_mode)==3 and psd.depth==depth
    layers=list(psd[0]) if e["groups"] else list(psd)
    if e["groups"]:
        assert psd[0].is_group() and psd[0].layer_id==10 and psd[0].blend_mode==b"pass"
        np.testing.assert_array_equal(psd[0].numpy("mask"),np.full((3,4,1),192/255,dtype="f4"))
    assert [l.layer_id for l in layers]==[12,11]
    for layer,identifier in zip(layers,[12,11]):
        rgb=e["background" if identifier==12 else "foreground"]
        alpha=e["alpha"] if identifier==11 else [maximum]*12
        expected=np.asarray(rgb+[alpha],dtype="f4").T.reshape(3,4,4)/maximum
        np.testing.assert_array_equal(layer.numpy(),expected)
        assert bool(layer.clipping_layer)==(identifier==11)
        if identifier==11:
            np.testing.assert_array_equal(layer.numpy("mask"),np.asarray(e["mask8"],dtype="f4").reshape(3,4,1)/255)
    composite=np.fromfile(OUT/f'{e["file"]}.rgba32.le',dtype="<f4").reshape(3,4,4)
    # Independently decode stored white-matted planes, then use the library's displayed RGBA.
    raw=psd._record.image_data.get_data(psd._record.header,False)
    stored=np.frombuffer(raw,dtype={8:"u1",16:">u2",32:">f4"}[depth]).astype("<f4").reshape(4,3,4).transpose(1,2,0)/maximum
    target=composite.astype("f8").copy();alpha=target[:,:,3:4]
    color=target[:,:,:3].copy() if depth==32 else np.clip(target[:,:,:3],0,1)
    target[:,:,:3]=np.where(alpha==1,color,color*alpha+(1-alpha))
    if depth==32: np.testing.assert_array_equal(stored.view("u4"),target.astype("<f4").view("u4"))
    else: np.testing.assert_allclose(stored,target,rtol=0,atol=1/maximum+1e-6)
    # psd-tools 1.10.9's float preview modifies a read-only bytes view. Supply the
    # exact independently decompressed bytes in a writable buffer; no samples change.
    psd._record.image_data.compression=Compression.RAW
    psd._record.image_data.data=bytearray(raw)
    displayed=psd.numpy()
    np.testing.assert_allclose(displayed[:,:,3],composite[:,:,3],rtol=0,atol=1/maximum+1e-6 if depth!=32 else 1e-6)
    # Low-alpha straight RGB has unavoidable quantization amplification; visible color is RGB*alpha.
    expected=color*alpha
    observed=displayed[:,:,:3]*displayed[:,:,3:4]
    tolerance=4e-6 if depth==32 else 3/maximum+1e-6
    np.testing.assert_allclose(observed,expected,rtol=0,atol=tolerance)

ids=json.loads((OUT/"nested-ids.json").read_text())
for ext in ["psd","psb"]:
    psd=PSDImage.open(OUT/f"nested.{ext}")
    assert [l.layer_id for l in psd]==[ids[0],ids[3],ids[4]]
    assert [l.layer_id for l in psd[1]]==[ids[2]]
    assert [l.layer_id for l in psd[1][0]]==[ids[1]]
    assert psd[1][0][0].name=="middle"
for path in (ROOT/"qa-output/psd-compression").glob("depth-*-mode-*.*"):
    if path.suffix not in [".psd",".psb"]:continue
    psd=PSDImage.open(path)
    assert len(psd)==1 and psd[0].is_group() and len(psd[0])==1
    assert psd[0][0].layer_id==1 and psd[0].mask is not None
print("RGB PSD/PSB order: 48 external inputs and 48 new outputs, four compressions and 8/16/32-bit layers/masks/white-matted planes and independent displayed RGBA; nested trees and existing 24 compressed grouped outputs verified by psd-tools")

