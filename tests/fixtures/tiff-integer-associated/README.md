Synthetic associated RGB UINT8/16 TIFF inputs generated independently by
NumPy/tifffile 2025.6.11 using `generate_tiff_integer_associated.py`.
Every source SHA-256 is pinned. No external image content.

96 inputs cover two depths, absent/working/custom matrix+gamma ICC from the
RGB float corpus, contiguous/separate planes, classic/BigTIFF, both byte
orders, raw/Deflate and three orientations. Raw associated components contain
fractional and HDR unassociation ratios; zero-alpha components are zero.
Recovered ratios are retained in a float32 working layer instead of rounding
them back to UINT8/16. Project and automatic TIFF output retain these floats.
