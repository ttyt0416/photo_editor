# Independent PCX fixtures

These small 5×3 RGB, palette, monochrome, and grayscale images were generated locally with Pillow 12.1.0. The matching PNG files contain the expected decoded pixels. They are input fixtures, separate from generated `qa-output/pcx` outputs. No external photograph or third-party artwork is included.

The Rust decoder is compared with all four inputs. The Rust writer generates `qa-output/pcx/rust-export.pcx`, which is also decoded independently with Pillow and compared with its expected pixels. Legacy and nonstandard PCX variants remain separate compatibility work.
