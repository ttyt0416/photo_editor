# TIFF strip, tile and predictor references

864 synthetic TIFFs from `../generate_tiff_layout.py`, a standalone classic and
BigTIFF IFD writer. zlib compresses independently encoded byte-plane Predictor3
for float16/64 or integer Predictor2 for UINT32; equivalent inputs without
prediction are also included. Dimensions 17x19 require right/bottom partial
16x16 tiles or three strips with multiple rows. Planar/interleaved, both byte
orders, gray/RGB, alpha association and white gray are covered. SHA-256 pins all
inputs; original working ICC and XMP test metadata handling.

`../verify_tiff_layout.py` reads metadata with tifffile, decompresses with zlib
and uses NumPy cumulative sums to reverse prediction. It reconstructs/crops
chunks independently before f64 inversion/alpha division and f32 conversion,
then compares project metadata/CRC and OpenCV EXR/TIFF output bits. Input
precision conversion follows steps137–139, not native64/UINT32 layer storage.
