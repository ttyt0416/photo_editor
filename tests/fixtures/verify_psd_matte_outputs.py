"""Independent white-matte storage/display and native flat import checks."""
from pathlib import Path
import json,hashlib
import numpy as np
from psd_tools import PSDImage
from psd_tools.constants import Compression
ROOT=Path(__file__).resolve().parents[2];FIX=ROOT/"tests/fixtures/psd-matte";OUT=ROOT/"qa-output/psd-matte"
entries=json.loads((FIX/"manifest.json").read_text());assert len(entries)==48
for e in entries:
    name=e["file"];depth=e["depth"];maximum={8:255,16:65535,32:1}[depth]
    assert hashlib.sha256((FIX/name).read_bytes()).hexdigest()==e["sha256"]
    if e["legacy_file"]:assert hashlib.sha256((FIX/e["legacy_file"]).read_bytes()).hexdigest()==e["legacy_sha256"]
    expected=np.asarray(e["source"],dtype="<f4").reshape(2,6,4)
    imported=np.fromfile(OUT/f"{name}.input.rgba32.le",dtype="<f4").reshape(2,6,4)
    tolerance=4e-6 if depth==32 else 3/maximum+1e-6
    np.testing.assert_allclose(imported[:,:,:3]*imported[:,:,3:4],expected[:,:,:3]*expected[:,:,3:4],rtol=0,atol=tolerance)
    np.testing.assert_allclose(imported[:,:,3],expected[:,:,3],rtol=0,atol=1e-6 if depth==32 else 1/maximum+1e-6)
    psd=PSDImage.open(OUT/name);assert psd.depth==depth and int(psd.color_mode)==3
    np.testing.assert_array_equal(psd[0].numpy().astype("<f4").view("u4"),expected.view("u4"))
    source=np.fromfile(OUT/f"{name}.source.rgba32.le",dtype="<f4").reshape(2,6,4)
    raw=psd._record.image_data.get_data(psd._record.header,False)
    stored=np.frombuffer(raw,dtype={8:"u1",16:">u2",32:">f4"}[depth]).astype("<f4").reshape(4,2,6).transpose(1,2,0)/maximum
    target=source.astype("f8").copy();color=target[:,:,:3].copy();alpha=target[:,:,3:4]
    target[:,:,:3]=np.where(alpha==1,color,color*alpha+(1-alpha))
    if depth==32:np.testing.assert_array_equal(stored.view("u4"),target.astype("<f4").view("u4"))
    else:np.testing.assert_allclose(stored,target,rtol=0,atol=1/maximum+1e-6)
    # Writable raw view adapts the installed reader's float read-only view defect only.
    psd._record.image_data.compression=Compression.RAW;psd._record.image_data.data=bytearray(raw)
    shown=psd.numpy()
    np.testing.assert_allclose(shown[:,:,:3]*shown[:,:,3:4],source[:,:,:3]*source[:,:,3:4],rtol=0,atol=tolerance)
    assert np.all(stored[:,:,:3][source[:,:,3]==0]==1)
print("PSD/PSB white matte: 48 independent native flat imports + 24 legacy inputs; 48 native outputs preserve exact layer bits and independently match stored matte and visible RGBA, including tiny alpha/HDR/negative/opaque/transparent values")
