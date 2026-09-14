# TrueRenderer

TrueRenderer is a native desktop image browser and viewer designed to make the rendering path traceable: linear-light processing in 32-bit floating point, physical-pixel viewing, and explicit decoder and colour provenance.

**Version 0.1.5 · development prototype · proprietary license.** This public repository is available for inspection; rights to original project material are reserved by Antonio Dicorato. Use, modification, and distribution require permission under [LICENSE](LICENSE), subject to applicable law, GitHub's terms, and third-party licenses. This is not an open-source project.

![TrueRenderer displaying generated fixtures on the historical macOS build](reports/10-external-grid.png)

## Current status

The Windows port, selectable RAW engines and subsequent fixes were published in commit `67146e3` on 14 September 2026. The latest Windows campaign passed **107 Rust tests (99 ordinary tests and 8 integrations)**, debug/release builds, colour/orientation regressions and IPC checks. See the [corrections and limits](docs/revisione-aggiuntiva-2026-09-14.md#correzioni) and [report with hashes](reports/gamma-orientation-fixes-windows.json).

The last verified macOS bundle is the **9 September baseline**, recorded in the [macOS verification report](reports/preview-navigation-continuation-macos.json). Native build and XPC verification of the new LibRaw/engine configuration remain pending. Earlier photographic and GUI campaigns apply to the binaries identified in their own reports; they were not repeated for the latest gamma/orientation fixes.

Every output still has **Preview** assurance. Qualified Standard/Reference modes, complete colour/display qualification, clean Windows installation testing and the general R0–R4 gates remain open.

## Features

- Open images and folders using dialogs, drag and drop, or `--open`.
- Browse a thumbnail grid, filmstrip and metadata inspector; search and filter the catalog.
- Fit, zoom, pan, view physical 1:1 and compare two images with synchronized views.
- Choose Standard or Full preview quality, globally or per photo. These resolution choices are separate from assurance: both remain Preview.
- Inspect decoder, colour and orientation provenance, source SHA-256, pixel values and a histogram labelled with the resident image stage.
- Rate, reject, label and add keywords; undo within a session and export annotations as JSON.
- Configure memory, GPU, CPU and disk-cache limits; pause background preparation or rebuild previews.

Original photographs are read-only. Annotations are stored separately in SQLite, with verified backups. No account or image upload is required.

## Platforms and formats

| Platform | External decoding | Current scope |
|---|---|---|
| Windows x86-64 | Worker confined with LPAC and a Job Object | JPEG, PNG and TIFF through the portable bitmap decoder; RAW through LibRaw and the experimental Rust engine |
| macOS | App bundle with two sandboxed XPC services | The verified September 9 baseline uses ImageIO/ColorSync for bitmaps and CIRAWFilter for RAW, with generated JPEG, PNG, TIFF, HEIC, WebP, GIF, BMP and DNG checks |

On Windows, keep `tr-worker.exe` beside `truerenderer.exe`. External decoding requires the confined worker. The controlled pipe path retains its twelve-image analytic PNG allowlist. On macOS, external files require the XPC app bundle. See [Windows isolation](docs/adr/0009-isolamento-worker-windows.md) and [macOS isolation](docs/adr/0002-xpc-decoder-r0.md).

Format recognition does not guarantee support for every variant. Windows rejects unsupported bitmap colour declarations instead of claiming full ICC support. Decoder limits and supported subsets differ by platform; large images may also exceed the selected memory budget. AVIF, JPEG XL, EXR and PSD are not enabled. The [architecture](docs/TrueRenderer-Architettura.md) and [verification history](reports/VERIFICA.md) record the contracts and measured cases.

### RAW engines

In **Impostazioni → Motore RAW**, choose an engine and press **Applica e salva**. Windows offers LibRaw bilinear (default), LibRaw AHD and experimental TrueRenderer fp32. The macOS build configuration also offers Apple RAW (default); that updated bundle still needs native validation. Applying the setting refreshes previews while preserving selection and zoom, and cache identities distinguish the recipes.

LibRaw 0.22.2 reads the RAW data. Bilinear and AHD development pass through a 16-bit integer RGB raster; the independent Rust demosaic develops the mosaic in fp32 without an integer RGB intermediate. The Rust engine currently accepts Nikon D750 and D40 Bayer NEFs and the project's synthetic Bayer DNG fixtures. Other cameras and variants are not implicitly supported.

Earlier Windows campaigns developed 30 D750 and 22 D40 NEFs with all three engines, preserving the originals. These functional checks do not establish camera colour accuracy or superiority over AHD or Apple. The historical bilinear path can deliver an explicitly labelled embedded preview where supported; an explicit experimental engine is not silently replaced by another engine. See the [RAW engine design, recipes and results](docs/progetto-motori-raw.md).

## Rendering and cache

The working space is extended linear Rec.2020, fp32, with premultiplied alpha. Output is currently opaque sRGB8. Aligned physical 1:1 maps source samples to device pixels without filtering; reduction uses bandwidth-guarded Lanczos3, enlargement uses Mitchell, and alpha filtering uses nonnegative weights. CPU/GPU comparisons check rendering arithmetic; they do not certify ICC handling or a monitor. See the [sampling criteria](docs/adr/0003-campionamento-fisico-r0.md).

Writable image folders can contain a `.truerenderer-cache/` directory with an ownership marker, locks, derived preview entries and temporary files. Cache records preserve fp32 bits and are keyed by source hash and pipeline/recipe identity. They contain no ratings, keywords or backups. Unavailable storage or a skipped write leaves rendering available in memory; full cache reuse under Windows lock contention remains unqualified.

Standard preview quality currently derives levels up to 2048 pixels from a temporary native decode. Full retains the levels required by the view, including source detail at physical 1:1. These controls do not provide regional or gigapixel RAW decoding. Admission accounting and platform isolation limits are distinct; historical macOS RSS measurements and ad hoc signing do not establish release guarantees. Detailed settings, scheduling and remaining performance gates are in the [preview/cache specification](docs/progetto-anteprime-cache-prestazioni.md).

## Building and running

Run commands from the repository root. The repository pins Rust **1.98.1** in [rust-toolchain.toml](rust-toolchain.toml); use [scripts/cargo-local.sh](scripts/cargo-local.sh), which selects the local `.tools/` installation when present. Python 3 is used by fixture and verification scripts. Fetch dependencies before using `--offline`.

### Windows development build

Requires an MSVC C++ build environment with the Windows SDK and a Bash shell, such as MSYS2. From PowerShell, with those tools available:

```powershell
bash scripts/cargo-local.sh fetch --locked
bash scripts/cargo-local.sh build --workspace --locked --offline
.\target\debug\truerenderer.exe
# Or open a file:
.\target\debug\truerenderer.exe --open "C:\Photos\image.jpg"
```

Basic source checks:

```powershell
bash scripts/cargo-local.sh fmt --all --check
bash scripts/cargo-local.sh clippy --workspace --all-targets --locked --offline -- -D warnings
bash scripts/cargo-local.sh test --workspace --locked --offline
```

The ordinary test command does not run ignored worker/native integrations. The [latest correction notes](docs/revisione-aggiuntiva-2026-09-14.md#correzioni) identify the additional Windows checks and their scope. [verify.sh](scripts/verify.sh) currently uses macOS executable paths and native checks; it is not a complete Windows verification command.

### macOS development bundle

Requires Xcode Command Line Tools, Python 3 and the pinned Rust toolchain. Generating the WebP fixture also requires `cwebp`. Native GUI checks require a desktop session.

```sh
./scripts/cargo-local.sh fetch --locked
python3 scripts/generate-format-fixtures.py
./scripts/verify.sh
./scripts/build-macos.sh
python3 scripts/test-xpc-integration.py
open -n dist/TrueRenderer.app --args --open "/path/to/image.jpg"
```

`./scripts/verify.sh --gui` adds native presentation checks. After broker, decoder or bundle changes, test both XPC services on the newly built package. These commands describe the workflow; the updated Mac bundle has not yet passed it. The historical arm64 bundle was tested on Apple M4/macOS 26.6.2 with an ad hoc signature, not notarization.

## Local data and documentation

Keep `var/library.sqlite` and `var/backups/`: they are durable user data, not disposable caches. Settings live in `var/settings.json`. Private photographs, working reports, toolchains, build outputs and caches stay outside Git.

Start with the [documentation index](docs/README.md), which distinguishes current specifications, historical reports and retained copies. The [development plan](PLAN.md) tracks work and gates; [avanzamento](docs/avanzamento.md) is the editable progress register. After changing that register or the integrated preview specification, run:

```sh
python3 scripts/sync-docs.py
```

This updates managed sections in both architecture documents. Preserve the original v1.2 backup and user text outside those sections.

## Dependencies and trademarks

See [NOTICE.md](NOTICE.md) for dependency inventories, LibRaw sources/notices and trademarks. TrueRenderer is independent of the companies and projects named here; naming a format or camera does not imply affiliation, certification or universal compatibility.
