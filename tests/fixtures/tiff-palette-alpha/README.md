# Palette alpha TIFF input references

320 independently written inputs from `../generate_tiff_palette_alpha.py`,
pinned by SHA-256. Each pixel contains an unsigned index and alpha of the same
1/2/4/8/16-bit width; packed, interleaved or planar storage covers both TIFF
structures and byte orders, multirow strips, partial tiles, raw and Deflate.
Three ICC choices and three orientations cycle through the matrix.

ColorMap values are interpreted as stored premultiplied RGB for associated
alpha. Indices are never multiplied by alpha. Arbitrary associated table values
include recovered HDR; alpha zero becomes RGB zero. Straight alpha retains hidden
palette color in the float32 layer. The oracle starts with actual stored RGB
tables and alpha, not an assumed original unassociated source color.

`../verify_tiff_palette_alpha.py` reconstructs chunks using NumPy unpackbits,
checks known index/alpha patterns, divides RGB table values in float64 then
rounds to float32, checks ICC math and project metadata/CRC, and decodes EXR
with OpenCV. It compares automatic working TIFF/EXR floats exactly. Native
indexed editing/output, other sample layouts and complete TIFF conformance
remain goals; palette-alpha input maps to float32 working layers.
