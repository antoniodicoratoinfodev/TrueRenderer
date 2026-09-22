# egui-wgpu local integration

Upstream: egui-wgpu 0.36.1, egui commit
`4c1f2fae95475a40e524884ebb298bcb1714b08e`, from the crates.io package.
Original authors and dual MIT OR Apache-2.0 licensing are unchanged; the full
upstream texts accompany this copy in LICENSE-MIT and LICENSE-APACHE.
This directory is excluded from TrueRenderer's proprietary licensing terms.

Local modifications (22 September 2026):

- `src/lib.rs`: optional creation-time surface format, advertised format/colour
  capabilities and effective fallback diagnostics.
- `src/winit.rs`: select only a supported requested format paired with explicit
  sRGB colour space; no automatic HDR/scRGB interpretation.
- `src/capture.rs`: decode 10-bit and half-float captures for illustrative PNGs;
  optional one-frame native-code capture for numerical presentation diagnostics.
- `Cargo.toml`: package includes refer to the accompanying local licenses.

The production default remains the upstream 8-bit path. The application requests
SDR10 or SDR16F explicitly; unsupported sRGB pairs fall back to 8-bit. These
changes do not establish the depth of the physical display or qualify HDR.
Registry download markers and the dependency's redundant lockfile are omitted.
The original manifest and upstream revision metadata are retained for provenance.
