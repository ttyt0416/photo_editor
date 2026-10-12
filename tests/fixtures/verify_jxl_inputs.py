"""Verify pinned libjxl inputs and independently compare editor output with reference floats."""
from pathlib import Path
import array, ast, hashlib, json, math, struct, sys, zlib
ROOT = Path(__file__).resolve().parent / "jxl"
OUTPUT = ROOT.parents[2] / "qa-output" / "jxl"
manifest = json.loads((ROOT / "manifest.json").read_text(encoding="utf-8"))
measurements = json.loads((OUTPUT / "measurements.json").read_text(encoding="utf-8"))
for name, entry in manifest["files"].items():
    data = (ROOT / name).read_bytes()
    assert len(data) == entry["size"] and hashlib.sha256(data).hexdigest() == entry["sha256"], name
    if name.endswith(".zlib"):
        original = zlib.decompress(data)
        assert len(original) == entry["original_size"]
        assert hashlib.sha256(original).hexdigest() == entry["original_sha256"], name
        descriptor = json.loads((ROOT / name.split("/")[0] / "test.json").read_text())
        assert entry["original_sha256"] == descriptor["sha256sums"]["reference_image.npy"]
results = {}
for name, info in manifest["cases"].items():
    ref = zlib.decompress((ROOT / name / "reference_image.npy.zlib").read_bytes())
    assert ref[:8] == b"\x93NUMPY\x01\x00"
    offset = 10 + struct.unpack_from("<H", ref, 8)[0]
    header = ast.literal_eval(ref[10:offset].decode("ascii"))
    assert list(header["shape"]) == info["shape"] and header["descr"] == "<f4" and not header["fortran_order"]
    reference = array.array("f"); reference.frombytes(ref[offset:])
    output = array.array("f"); output.frombytes(zlib.decompress((OUTPUT / f"{name}.rgba32.le.zlib").read_bytes()))
    if sys.byteorder != "little": reference.byteswap(); output.byteswap()
    _, h, w, channels = info["shape"]
    assert len(reference) == h*w*channels and len(output) == h*w*4
    descriptor = json.loads((ROOT / name / "test.json").read_text())
    depth = measurements[name]["depth"]
    quant = {8: 0.5/255, 16: 0.5/65535, 32: 0}[depth]
    peaks = [0.0]*channels; sums = [0.0]*channels
    for i in range(h*w):
        if channels != 4: assert output[i*4+3] == 1
        for c in range(channels):
            actual = output[i*4+c]
            if name == "alpha_premultiplied" and c < 3: actual *= output[i*4+3]
            expected = reference[i*channels+c]
            assert math.isfinite(actual) and math.isfinite(expected)
            difference = abs(actual - expected)
            peaks[c] = max(peaks[c], difference); sums[c] += difference*difference
    rmses = [math.sqrt(v/(h*w)) for v in sums]
    for c in range(channels):
        peak = descriptor["frames"][0]["peak_error"] + quant
        rms = descriptor["frames"][0]["rms_error"] + quant
        if name == "alpha_premultiplied" and c < 3:
            # This is the decoder project's separate VarDCT RGB bound, not the corpus's common alpha bound.
            peak, rms = 0.004, 0.0001
        assert peaks[c] <= peak+1e-6 and rmses[c] <= rms+1e-7, (name,c,peaks,rmses)
    results[name] = {"depth":depth,"shape":[h,w,channels],"peak":peaks,"rms":rmses,
                     "corpus_common_peak_pass":max(peaks)<=descriptor["frames"][0]["peak_error"]+quant+1e-6}
(OUTPUT / "independent-verification.json").write_text(json.dumps(results,indent=2),encoding="utf-8")
print(f"{len(results)} JPEG XL references: hashes, all float samples, alpha, dimensions and precision verified")
print("alpha_premultiplied RGB uses upstream VarDCT limits; the corpus common alpha limit is not claimed.")
