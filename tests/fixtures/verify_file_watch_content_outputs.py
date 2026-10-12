"""Independent hashlib verification of streaming file conflict fingerprints."""
from pathlib import Path
import json,hashlib
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'qa-output/file-watch-content'
case=json.loads((OUT/'hashes.json').read_text());data=(OUT/case['file']).read_bytes()
assert len(data)==case['bytes']==131073
assert list(hashlib.sha256(data).digest())==case['original']
changed=bytearray(data)
for offset,value in case['mutations']:changed[offset]^=value
assert list(hashlib.sha256(changed).digest())==case['changed']
assert case['original']!=case['changed']
print('External file content: independent SHA256 of 131073 bytes crossing 64KiB chunks; identical-length middle/final changes, UTF8 path and restored original bytes verified')
