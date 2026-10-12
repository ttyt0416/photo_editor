# Float32 RGB planar TIFF references

144 independently encoded inputs from `../generate_tiff_rgb_planar_float.py`,
pinned by SHA-256. RGB and straight/associated RGBA planes cover classic/BigTIFF,
both byte orders, multirow strips, partial tiles and Deflate with/without
floating-point prediction. Three ICC choices and three orientations include
negative/HDR samples, signed zero, minimum float32 subnormal and fractional alpha.

`../verify_tiff_layout.py --planar-float32` reconstructs all planes with zlib
and NumPy cumsum; three native tifffile baselines verify raw chunks. It checks
RGB unassociation, ICC math, exact identity/alpha and project metadata/CRC,
then uses OpenCV for project/output EXR and compares working TIFF floats.
The app uses full-plane reading; the old TIFF read_image API reads only one
plane. Existing non-planar float32 RGB paths remain supported by the generic
adapter. UINT8/16 RGB planar and other TIFF/ICC variants remain goals.
