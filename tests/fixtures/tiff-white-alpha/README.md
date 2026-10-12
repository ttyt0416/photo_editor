Synthetic WhiteIsZero gray+alpha TIFF corpus independently encoded with
NumPy/tifffile 2025.6.11 using `generate_tiff_white_alpha.py`.
All source SHA-256 values are pinned. No external image content.

384 inputs cover UINT8/16 and FLOAT32, absent/linear/gamma1.8/working GRAY ICC,
associated/straight alpha, contiguous/separate planes, classic/BigTIFF, both
byte orders, raw/Deflate and three orientations. Floating samples encode
negative/HDR black values as white complements before association. Integer
associated samples also exercise recovered negative ratios. Inversion and
unassociation are compared against the actual stored sample values; storage
rounding and lost signed-zero information in `1 - black` are accounted for.
