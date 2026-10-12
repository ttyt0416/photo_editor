Local TIFF patches used by Photo Editor (crate remains pinned at 0.11.3):

- Existing floating-point predictor handling supports the app's exact f32
  TIFF output and independently verified byte-plane round trips.
- `Decoder::read_image_to_buffer_uninverted` reads samples without applying
  WhiteIsZero inversion. It uses the same compression/prediction/layout/limits
  and restores the interpretation on success or error. The app requests this
  explicitly for white-zero grayscale+alpha and performs inversion together
  with alpha unassociation in f64 before storing f32. This avoids unsupported
  multiband inversion and unsigned underflow for negative recovered values.
  Ordinary decoder calls keep their existing behavior. Original IFD values and
  source file bytes are never modified. Tests exercise restoration on both
  successful reads and allocation-limit errors.
- Tile edge padding uses the chunk index within each plane. The upstream
  absolute chunk index lost bottom padding after the first plane, causing an
  assertion failure when decoding planar images with partial bottom tiles.
  The fix preserves file chunk offsets and predictor/compression handling.
  Direct per-plane dimensions and 864 independent strip/tile float16/64 and
  UINT32 cases exercise all planes and right/bottom edges (step140).
