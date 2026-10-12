"""Compare every Rust-imported grayscale sample against independent fixtures/psd-tools."""
from pathlib import Path
import json, hashlib
import numpy as np
from psd_tools import PSDImage
ROOT=Path(__file__).resolve().parents[2]
FIX=ROOT/"tests/fixtures/gray-psd"
OUT=ROOT/"qa-output/gray-psd"
entries=json.loads((FIX/"manifest.json").read_text(encoding="utf-8"))
assert len(entries)==72
checked=0
for e in entries:
    path=FIX/e["file"]
    assert hashlib.sha256(path.read_bytes()).hexdigest()==e["sha256"]
    psd=PSDImage.open(path)
    assert int(psd.color_mode)==1 and psd.depth==e["depth"]
    maximum={8:255,16:65535,32:1}[e["depth"]]
    targets=[(12,e["gray"],[maximum]*12),(11,e["foreground"],e["alpha"])] if e["kind"]=="layers" else [(1,e["merged"],e["merged_alpha"])]
    for identifier,gray,alpha in targets:
        target=np.asarray([gray,gray,gray,alpha],dtype="f4").T/np.float32(maximum)
        actual=np.fromfile(OUT/f'{e["file"]}-{identifier}.rgba32.le',dtype="<f4").reshape(12,4)
        np.testing.assert_array_equal(actual.view("u4"),target.astype("<f4").view("u4"))
        source=psd[0][0 if identifier==12 else 1].numpy() if e["kind"]=="layers" else psd.numpy()
        np.testing.assert_array_equal(source.reshape(12,-1)[:,0],target[:,0])
        checked+=1
for depth in [8,16,32]:
    rgb=PSDImage.open(OUT/f"neutral-{depth}.psd")
    assert int(rgb.color_mode)==3 and rgb.depth==depth
    assert not any(int(identifier)==1039 for identifier in rgb.image_resources)
    e=next(e for e in entries if e["depth"]==depth and not e["large"] and e["compression"]==0 and e["kind"]=="flat-alpha")
    maximum={8:255,16:65535,32:1}[depth]
    target=np.asarray([e["merged"]]*3+[e["merged_alpha"]],dtype="f4").T.reshape(3,4,4)/maximum
    np.testing.assert_array_equal(rgb[0].numpy(),target)
print(f"Grayscale PSD/PSB: 72 independent 8/16/32-bit and four-compression inputs; {checked} Rust pixel arrays bit-exact; three neutral RGB exports preserve samples and omit incompatible ICC")
