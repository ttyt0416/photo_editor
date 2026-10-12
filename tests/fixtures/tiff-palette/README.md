# Palette TIFF input references

240 synthetic inputs from `../generate_tiff_palette.py`, pinned by SHA-256.
The independent IFD writer stores unsigned 1/2/4/8/16-bit indices, row-end
padding, and three full 16-bit ColorMap tables. It covers raw/Deflate,
classic/BigTIFF, both byte orders, multirow strips and right/bottom partial tiles,
orientation and original ICC/XMP. RGB color maps are never reduced to 8-bit.

`../verify_tiff_palette.py` reconstructs chunks with zlib and NumPy unpackbits,
checks a known index pattern, indexes tifffile's original uint16 ColorMap,
calculates ICC matrix/gamma/sRGB, checks project metadata/CRC and uses OpenCV
to inspect stored PNG16 layer assets. Working TIFF16 samples must be identical.

TIFF6 baseline palette indices are 4/8-bit. Other index widths here are extra
supported inputs, not a full TIFF conformance claim. Single-channel palettes are
supported; palette alpha, palette output, TransferFunction/colorimetry and other
codec variants remain goals. FillOrder1 is used; a separate Rust test checks
uncompressed 1-bit FillOrder2 and decoder state restoration after errors.
