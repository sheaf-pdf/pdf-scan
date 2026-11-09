# scan-fixtures

Layer: Tests (`crates/README.md`). Paths are from `docs/pm/`.

Never published, as pdf-fixtures isn't; crates take it by path as a dev-dependency.

- Seeded synthetic pages (`codes::fixture`): the M5 bench's 300 dpi Letter pages of text, rules and a table,
  the same on every run, on the fixtures' own xorshift generator. Never imageproc's noise: it seeds
  `StdRng`, whose output rand says may change across versions even with a fixed seed. Its JPEG, blur and
  resampling steps can still change bytes when `image` or imageproc moves: pinned, and said when bumped.
- The bench's degradations (`Degradation`): skew, quarter turns, Gaussian noise, salt and pepper, blur, low
  contrast, a JPEG round trip, a threshold to black and white, and resampling to a resolution. Gray only
  today, so the Photo profile's color steps can't be tested on them: RGB and Photo effects ported from
  Augraphy (MIT) to come (FEAT-8). No Rust library degrades document images.
- Codes drawn: QR, Data Matrix, Code 39, Code 128 and PDF417 (through rxing's encoders: Data Matrix input
  capped, since its writer panics past 144×144; Code 39 drawn from its pattern where the wide-to-narrow
  ratio matters, since every Rust encoder fixes it at 2:1), and patch codes as Kodak's and Alliance's sheets
  lay them out.
- G3 and Modified Huffman TIFFs for BUG-2, from libtiff run as a separate process (as image-tiff made its
  own), and libtiff's sample images `fax2d.tif` and `g3test.tif`. Nothing in Rust encodes G3 or MH.
- `fake` device scripts, for `scan-device`'s `fake` backend.
- Not the fake libsane: it was planned here, and S0 slice 1 made it `fake-libsane`, a crate of its own for
  good (`crates/fake-libsane.md`; `decisions.md`, "Made here"). It's a shared object tests load by its path,
  so it must carry none of the image codecs this crate takes, and no crate may depend on it.
- Licenses checked before any outside image is vendored: zxing-cpp's sample and false-positive sets,
  BarBeR, DIBCO, DISEC'13, DocUNet (TEST-5, TEST-6).
