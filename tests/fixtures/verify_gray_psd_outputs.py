"""Independently verify native grayscale output and both RGB conversion choices."""
from pathlib import Path
import json
import numpy as np
from psd_tools import PSDImage
ROOT=Path(__file__).resolve().parents[2]
def shown_raw(psd):
    raw=psd._record.image_data.get_data(psd._record.header,False)
    maximum={8:255,16:65535,32:1}[psd.depth]
    return np.frombuffer(raw,dtype={8:"u1",16:">u2",32:">f4"}[psd.depth]).astype("<f4").reshape(2,psd.height,psd.width).transpose(1,2,0)/maximum

def gray(rgb):
    data=rgb.astype("f8")
    return (data[:,:,0]*.2126+data[:,:,1]*.7152+data[:,:,2]*.0722).astype("<f4")

entries=json.loads((ROOT/"tests/fixtures/gray-psd/manifest.json").read_text(encoding="utf-8"))
for e in entries:
    name=e["file"];psd=PSDImage.open(ROOT/"qa-output/gray-psd-export"/name);depth=e["depth"];maximum={8:255,16:65535,32:1}[depth]
    assert int(psd.color_mode)==1 and psd.depth==depth and psd._record.header.channels==2
    assert psd.image_resources[1039].data[16:20]==b"GRAY"
    if e["kind"]=="layers":
        assert len(psd)==1 and psd[0].is_group() and [l.layer_id for l in psd[0]]==[12,11]
        targets=[(psd[0][0],e["gray"],[maximum]*12),(psd[0][1],e["foreground"],e["alpha"])]
        np.testing.assert_array_equal(psd[0][1].numpy("mask"),np.asarray(e["mask8"],dtype="f4").reshape(3,4,1)/255)
    else:targets=[(psd[0],e["merged"],e["merged_alpha"])]
    for layer,g,a in targets:
        expected=np.asarray([g,a],dtype="f4").T.reshape(3,4,2)/maximum
        np.testing.assert_array_equal(layer.numpy().astype("<f4").view("u4"),expected.astype("<f4").view("u4"))
    source=np.fromfile(ROOT/"qa-output/gray-psd-export"/f"{name}.rgba32.le",dtype="<f4").reshape(3,4,4)
    expected=source[:,:,[0,3]]
    if depth==32:np.testing.assert_array_equal(shown_raw(psd).view("u4"),expected.copy().view("u4"))
    else:np.testing.assert_allclose(shown_raw(psd),np.clip(expected,0,1),rtol=0,atol=1/maximum+1e-6)

entries=json.loads((ROOT/"tests/fixtures/rgb-psd-order/manifest.json").read_text(encoding="utf-8"))
for e in entries:
    name=e["file"];depth=e["depth"];maximum={8:255,16:65535,32:1}[depth]
    source=np.fromfile(ROOT/"qa-output/gray-psd-color"/f"{name}.source.rgba32.le",dtype="<f4").reshape(3,4,4)
    for keep in [False,True]:
        psd=PSDImage.open(ROOT/"qa-output/gray-psd-color"/f"keep{int(keep)}-{name}")
        assert int(psd.color_mode)==1 and psd.depth==depth and 1039 not in psd.image_resources
        if keep:
            layers=list(psd[0]) if e["groups"] else list(psd);assert [l.layer_id for l in layers]==[12,11]
            for layer,identifier in zip(layers,[12,11]):
                rgb=np.asarray(e["background" if identifier==12 else "foreground"],dtype="f4").T.reshape(3,4,3)/maximum
                expected=gray(rgb)
                actual=layer.numpy("color")[:,:,0]
                if depth==32:np.testing.assert_array_equal(actual.astype("<f4").view("u4"),expected.view("u4"))
                else:np.testing.assert_allclose(actual,np.clip(expected,0,1),rtol=0,atol=1/maximum+1e-6)
                alpha=e["alpha"] if identifier==11 else [maximum]*12
                np.testing.assert_array_equal(layer.numpy("shape"),np.asarray(alpha,dtype="f4").reshape(3,4,1)/maximum)
        else:
            assert len(psd)==1 and not psd[0].is_group()
            expected=np.stack([gray(source[:,:,:3]),source[:,:,3]],axis=2)
            if depth==32:np.testing.assert_array_equal(shown_raw(psd).view("u4"),expected.view("u4"))
            else:np.testing.assert_allclose(shown_raw(psd),np.clip(expected,0,1),rtol=0,atol=1/maximum+1e-6)
psd=PSDImage.open(ROOT/"qa-output/gray-psd-adjustments/adjusted.psd")
source=np.fromfile(ROOT/"qa-output/gray-psd-adjustments/source.rgba32.le",dtype="<f4").reshape(3,4,4)
np.testing.assert_allclose(shown_raw(psd),np.stack([gray(source[:,:,:3]),source[:,:,3]],axis=2),rtol=0,atol=1/255+1e-6)
print("Native grayscale PSD/PSB: 72 exact gray-source outputs, 96 colored composite/per-layer conversions and adjusted color result; independent profile/structure/mask/alpha/depth/gray formula checks passed")
