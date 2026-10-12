# Local changes to tiff 0.11.3

Source: the crates.io tiff 0.11.3 package, retaining its MIT LICENSE and source.

Enable TIFF Predictor 3 in the encoder for uniform IEEEFP 16/32/64-bit samples. Each strip uses a bounded predicted byte buffer: native samples become most-significant-byte planes, then channel-stride byte differencing proceeds backwards across each complete row. Integer/unsupported formats remain rejected before writing. Both predictors are allowed only with LZW/Deflate to match common readers; uncompressed/PackBits stay available without prediction. Existing metadata, codecs and decoder are unchanged.

The algorithm is based on TIFF floating-point prediction and cross-checked against the existing decoder and primary libtiff fpDiff implementation: https://gitlab.com/libtiff/libtiff/-/blob/5e18004500cda10d9074bdb6166b054e95b659ed/libtiff/tif_predict.c . The app validates 32-bit RGBA classic/BigTIFF with an independent byte-plane decoder and LZW/Deflate compression choices. Other vendored sample types are not app support claims.
