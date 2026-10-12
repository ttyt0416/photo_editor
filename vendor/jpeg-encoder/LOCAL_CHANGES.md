# Local JPEG encoder correction

Upstream: jpeg-encoder 0.7.1, https://github.com/vstroebel/jpeg-encoder.
Original MIT, Apache-2.0 and IJG notices remain in this directory.

`src/encoder.rs`: Huffman DC frequency counting now resets the predictor at
restart boundaries, matching the sequential and progressive writers. Previously,
optimized tables could omit a DC category needed after a restart, causing a debug
panic or invalid release output. Optimization always uses non-interleaved scans,
so one block is one MCU in this pass.

The application regression fixture covers all eight sampling factors with baseline
and progressive scans, with and without optimization, and a restart interval of 3.
Rust and Pillow independently decode the resulting files. Preserve these checks
when replacing this patch with an upstream release.

The AC scan partition also uses 63 AC coefficients instead of 64, avoiding an
empty invalid first AC scan at the maximum 64 total scans. Frequency counting and
writing share the same nonempty intervals. Pillow rejects the previous output.
