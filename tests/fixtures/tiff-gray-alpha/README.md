Synthetic float32 grayscale+alpha TIFF corpus generated independently with
NumPy/tifffile 2025.6.11 by `generate_tiff_gray_alpha.py`. Input SHA-256 values
are pinned in the manifest. No external image content.

96 combinations cover absent/linear/working GRAY ICC, associated/unassociated
alpha, contiguous/separate planes, classic/BigTIFF, two byte orders, raw/Deflate
strips and three orientations. Gray includes negative/HDR samples to 1e8 and
alpha includes zero/signed zero/fractional/one. Black-zero gray is used.
Associated zero-alpha color is restored to zero; straight-alpha hidden gray
remains in the layer's float asset. Project composite/output follows the app's
existing zero-alpha RGB policy. White-zero alpha and other channels remain goals.
