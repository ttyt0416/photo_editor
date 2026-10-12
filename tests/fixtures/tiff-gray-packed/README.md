Synthetic 2/4-bit grayscale TIFF inputs authored by an independent Python IFD
writer and NumPy bit packing in `generate_tiff_gray_packed.py`. All SHA-256
values are pinned. No external image content.

256 files cover two depths, four GRAY ICC choices, classic/BigTIFF, both byte
orders, black/white zero, MSB/LSB FillOrder, raw/Deflate and three orientations.
Rows have 17 pixels to exercise trailing bit padding. Source XMP is UTF-8.
Profiles reuse the synthetic/working gray corpus. Verification independently
unpacks bits, applies gray transfer math and checks source/project metadata.
