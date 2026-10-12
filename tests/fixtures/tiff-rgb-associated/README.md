Synthetic RGB32F associated-alpha TIFF corpus independently encoded with
NumPy/tifffile 2025.6.11. SHA-256 values are pinned in the manifest.
Generate with `generate_tiff_rgb_associated.py`. No external image content.

48 combinations cover absent/working/custom matrix+gamma RGB ICC,
contiguous/separate planes, classic/BigTIFF, both byte orders, raw/Deflate and
three orientations. RGB includes different channels, negative values and HDR
to 1e8. Alpha includes zero/signed zero/fractions/one. Associated zero-alpha
RGB is restored to zero. The custom ICC modifies the app's working profile's
D50 XYZ columns and assigns signed gamma 2.2 curves; it is a synthetic profile,
not a claim to match a named standard gamut.
