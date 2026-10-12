# Local sequential JPEG correction

Upstream: zune-jpeg 0.5.15 from https://github.com/etemesi254/zune-image.
Original MIT, Apache-2.0 and Zlib notices remain in this directory.

- `src/headers.rs`: reset DC predictors and restart count at each SOS boundary.
- `src/mcu.rs`: non-interleaved baseline scans iterate the selected component's
  actual block rows and store each block row without an extra vertical sampling
  multiplier. The post-processing buffer retains its existing MCU stripe layout.

Application regression fixtures cover eight sampling factors, sequential and
progressive scans, optimization and restart markers. Compare output pixels with
independent Pillow/libjpeg decoding when replacing this local patch.

Removed the obsolete crate-level `macro_use` attribute, rejected by current compiler linting.
