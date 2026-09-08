# TrueRenderer

TrueRenderer is a native desktop image browser and viewer built around a single goal: showing a photograph exactly as its pixels describe it. The whole pipeline works in linear light at 32-bit floating point, the viewport draws at the display's real physical resolution, and every render can tell you where its pixels came from. It is made for photographers, retouchers, and anyone who has to trust the screen before making a decision about an image.

**Version 0.1.5 · development prototype · proprietary license.** This public repository is available for inspection; rights to original project material are reserved by Antonio Dicorato. Use, modification, and distribution require permission under [LICENSE](LICENSE), subject to applicable law, GitHub's terms, and third-party licenses. This is not an open-source project.

![TrueRenderer displaying generated image-format fixtures](reports/10-external-grid.png)

## Color and rendering fidelity

Color accuracy is the design goal. The implemented rendering path and its remaining qualification gaps are described below.

- **Linear-light working space.** Extended linear Rec.2020 in fp32 with premultiplied alpha. Resampling, compositing, and analysis all happen there, never in gamma-encoded 8-bit. Output is currently opaque sRGB8 on a neutral `#777777` surround.
- **Real physical pixels.** At aligned physical 1:1, each source sample maps to one physical pixel with no filtering at all. On a Retina display, 1:1 means device pixels, not logical points.
- **Physical-size rendering.** Grid, preview, comparison, and filmstrip use independently owned linear mip levels from the reference graph, then filter to the exact backing-pixel size. A thumbnail can remain available after the full source has been released. Full-quality thumbnails preserve the same graph as the full-resolution path.
- **Filters chosen per case.** Bandwidth-guarded Lanczos3 for opaque reduction, Mitchell for enlargement, nonnegative weights for alpha.
- **Full precision preserved.** 16-bit PNG and TIFF samples and RAW data are decoded at native precision and carried in fp32 throughout. EXIF orientation is applied exactly once.
- **RAW is actually developed.** CIRAWFilter runs at native resolution with the versioned `TR-linear-v1` recipe. An embedded JPEG preview is never silently substituted for the RAW data.
- **Traceable results.** The inspector reports the decoder, color and orientation information, pixel values and a histogram labelled with the resident level being analyzed, and the SHA-256 of the decoded source.

The actual separable WGSL filter is checked against scalar CPU before GPU computation is enabled on an adapter. The current generated test suite covers odd dimensions, transparent edges, fractional crops, physical 1:1 and signed linear values up to 1000, using `1e-5 + 1e-4 * abs(cpu)` per linear channel and at most one sRGB8 level at display output. See the [GPU quality report](reports/preview-quality-gpu-macos.json) and [native screenshot comparison](reports/sampling-presentation-macos.json). These are renderer checks, not ICC or monitor certification.

Two honest limits. Faithful reduction attenuates detail that the available pixels cannot represent, so a reduced view is not meant to look identical to 1:1; preserving that contrast would produce moiré. And every render currently carries the **Preview** status: qualified Standard and Reference pipeline modes, full ICC and monitor profile handling, and fidelity across different displays are not yet qualified. [ADR 0003](docs/adr/0003-campionamento-fisico-r0.md) records the sampling criteria and what remains open.

## What you can do with it

- Open single images or whole folders, by dialog, drag and drop, or the `--open` argument.
- Browse an adjustable thumbnail grid with a filmstrip and an inspector panel.
- Fit, zoom, pan, physical 1:1, and synchronized side-by-side comparison of two images.
- Choose Standard or Full preview quality globally, or Full for a single photo; physical 1:1 requests Full detail.
- Read the histogram and sample values with their resident-stage label, and check decoder, color, and orientation metadata.
- Set memory, disk, GPU and CPU limits; pause background preparation or rebuild folder previews.
- Rate, reject, label, and add keywords, then search and filter. Annotations live in a local SQLite library.
- Undo within a session, create verified SQLite backups, and export annotations as JSON.
- Work offline. No account, no image upload, no network access.

Original files are always read-only. Annotation saving runs in a writer separate from decoding, so a slow or failing decoder never blocks your edits.

## Supported formats

External image decoding requires the **macOS app bundle**, where decoding runs in two sandboxed XPC services. File contents determine the decoder; extensions are only used to discover images in a folder.

| Format | Decoder path | Tested subset |
|---|---|---|
| JPEG / JPG / JPE | ImageIO and ColorSync | RGB JPEG, including 4000×3000 pixels |
| PNG | ImageIO and ColorSync | RGB/RGBA, alpha, 16-bit precision |
| TIFF / TIF | ImageIO and ColorSync | RGB 8/16-bit, EXIF orientations 1–8 |
| RAW, including DNG | CIRAWFilter at native resolution | Generated 1024×768 Bayer RGGB DNG without an embedded JPEG |
| HEIC / HEIF | Operating-system decoder | RGB HEIC |
| WebP | Operating-system decoder | Lossless RGB WebP |
| GIF and BMP | ImageIO and ColorSync | Generated RGB images |

RAW compatibility depends on the **camera model and the installed Apple decoder**. Recognized extensions include DNG, NEF, NRW, CR2, CR3, CRW, ARW, SRF, SR2, RAF, ORF, RW2, RWL, PEF, SRW, 3FR, FFF, IIQ, MOS, MEF, MRW, ERF, and RAW. That list is not a guarantee that every camera or variant works. Unsupported files produce a visible error rather than a degraded render. See [ADR 0004](docs/adr/0004-formati-esterni-e-pubblicazione.md).

Limits: **256 MiB per source file**, **67,108,864 source pixels**, and **32,768 pixels per side**. Multi-page and animated containers show only their first page or frame, with up to 256 accepted. BigTIFF, CMYK and YCCK variants, complete ICC matrices, and cameras outside the tested subset still require qualification. AVIF, JPEG XL, EXR, and PSD are not enabled. Builds run outside the macOS bundle fall back to pipe workers and accept only the twelve included analytic PNG corpus images.

## Running the app

On the development Mac, open **`Avvia TrueRenderer.command`** or `dist/TrueRenderer.app`, then choose **Apri file…** (Open file) or **Apri cartella…** (Open folder), or drag an image into the window. You can also run:

```sh
open -n dist/TrueRenderer.app --args --open "/path/to/image.jpg"
```

The bundle uses the project's `corpus/` and `var/` folders, so move the complete project folder and keep the bundle at `dist/TrueRenderer.app`. Binaries, personal data, and generated caches stay out of Git. macOS handles any permission needed to reach your image folder.

This is an internal arm64 build with an ad hoc signature, tested on macOS 26.6.2 and Apple M4. It is not a notarized release. Minimum OS and GPU versions, and a real Windows target, remain to be qualified.

### Keyboard shortcuts

`G` grid, `E` preview, `C` comparison, arrow keys to change image. `Z` toggles Fit and 1:1, `Cmd+1` gives physical 1:1, and the wheel and dragging control zoom and pan. `0`–`5` set the rating, `X` marks a rejection, `6`–`9` apply labels. `Cmd+Z` undo, `Cmd+F` search, `I` inspector, `T` filmstrip, `F` fullscreen, `Esc` back to the grid. Edits and undo on a multi-selection currently apply per image rather than as one atomic batch.

## Lossless disk cache

Opening a writable image folder creates a hidden subfolder beside the photographs:

```text
Your image folder/
  .truerenderer-cache/
    OWNER                 identifies a disposable TrueRenderer cache
    cache.lock            coordinates readers, writers, and cleanup
    entries/              lossless v1 pyramids and v2 preview blocks/descriptors
    tmp/                  incomplete writes; cleaned after use or on reopening
```

In Finder, press **Cmd+Shift+.** to reveal hidden folders. The cache holds derived data only, never original photographs, ratings, keywords, or backups. The app will not claim an existing folder without its ownership marker, and it does not follow cache symlinks. A read-only folder, an unsupported filesystem operation, or a full disk falls back to rendering in memory with a status message.

The cache preserves fidelity exactly. Version 2 stores independent linear mip tails and their histogram, with fp32 bits intact. Payloads are split into records of at most 4 MiB; a bounded descriptor is published last. No JPEG compression or fp16 quantization is introduced. Version 1 entries remain within the same quota and can be read when their exact pipeline fingerprint is recognized and the full read fits the budget. A full SHA-256 of the source bytes is verified before a hit, and cache keys also include the app and pipeline version, the operating-system build, the working space, and the RAW recipe, so an edited file or a changed pipeline invalidates the entry. Disk hits skip decoding and pyramid construction entirely.

In the earlier 0.1.4 measurements on Apple M4, the hash optimization reduced the median warm load of a generated 12 MP JPEG from **0.963 s to 0.137 s** (7.0×), and a generated DNG from **0.0693 s to 0.00972 s** (7.1×). SHA-256 uses the library's hardware acceleration when supported, retaining its software fallback and all integrity checks. A cache miss reuses the same private source snapshot for decoding, avoiding a second file read and hash. These are local measurements of five warm loads with the OS cache left intact, not p95 guarantees; every pyramid level remains bit-identical. See the [before/after measurements](reports/cache-performance-comparison.json). The 0.1.5 pipeline delivers useful pixels before submitting optional persistence to a separate queue limited by bytes. Its [preview-cache report](reports/preview-cache-macos.json) measures independent thumbnails; those sizes and delivery timings should not be compared directly to the older full-pyramid loads as if they were the same work.

Open **Impostazioni** (Settings) in the toolbar to configure:

| Setting | Default | Behavior |
|---|---:|---|
| Preview quality | Standard for new data folders; Full when migrating | Both remain Preview assurance, with no qualified Standard badge |
| Total admission memory | `min(2 GiB, 25% physical RAM)` | Includes explicit base allowance, image leases, snapshots, decoder estimates and GPU work; not an OS-enforced RSS cap |
| Reusable RAM cache | Automatic, up to 40% of admission memory | Zero keeps working views but disables opportunistic CPU retention |
| GPU sublimit | Automatic, initially 256 MiB within total memory | Direct compute textures, source buffers and work in flight share credits |
| Compute | Automatic | Qualified GPU for sufficiently large work; CPU fallback for unsupported resources or limits; CPU can be selected explicitly |
| CPU profile | Performance, with battery adaptation enabled | Application thread limit; operating-system decoders have their own internal scheduling |
| Prefetch | Automatic | Bounded neighboring requests after navigation settles; extended or disabled options |
| Disk cache | Enabled | Can be disabled without deleting existing entries |
| Disk quota per image folder | 4,096 MiB (4 GiB) | Includes space reserved for an active temporary write |
| Maximum temporary artifact | 2,048 MiB (2 GiB) | Entries too large for either quota are rendered without being cached |
| Expiration after last use | 30 days | Checked when the folder is opened or cache space is managed |
| Free disk space reserve | 512 MiB | A write is skipped if it would consume the reserve |

**Applica e salva** (Apply and save) stores the settings and runs maintenance on the current folder; other folders adopt the new policy when reopened. **Svuota cache cartella** (Empty folder cache) removes recognized cache artifacts and abandoned temporary files from the current folder, leaving images already in memory available.

Least recently used entries are evicted to make room. New maintenance protects a recoverable 20% minimum for complete thumbnails and lends unused space to other classes. A previous app version still applies its own LRU policy, producing safe misses if it evicts newer records. Writes use a same-folder temporary file, a checksum, and atomic publication that never overwrites an unexpected destination, while interprocess locks keep cleanup away from active readers and writers. Cache files are validated as untrusted data, with bounded headers, dimensions, pixel values, and exact lengths.

Standard currently derives at most 2048-pixel levels from a temporary full native decode; Full keeps the levels required by the physical view, including source detail for 1:1. Apple contexts are reused, but native reduced RAW decoding and production Metal decoding remain unqualified. These controls do not add regional RAW or gigapixel decoding. A source that cannot fit the selected admission limit produces an explicit memory error; increasing quality never silently bypasses the limit. Uncompressed fp32 data can be much larger than the original JPEG or RAW. Free-space checks also cannot reserve space against other applications writing to the same disk. [ADR 0005](docs/adr/0005-cache-cartella.md) and [ADR 0006](docs/adr/0006-anteprime-residenza-compute.md) record the design and remaining qualification gates.

Visible work uses seven priority classes with FIFO ordering and promotion of pending requests. Prefetch follows navigation direction with a bounded neighborhood and an adaptive delay. On macOS, memory-pressure notifications suspend speculative work and trim reusable CPU/GPU storage while active buffers keep their admission credits. Cache writers yield to waiting local readers between bounded batches.

## Building and testing

For the owner and authorized developers: macOS arm64, Xcode Command Line Tools, Python 3, and Rust via rustup. `rust-toolchain.toml` pins Rust 1.98.1 and `Cargo.lock` pins dependencies. A local `.tools/` toolchain takes precedence when present.

```sh
./scripts/cargo-local.sh fetch --locked  # initial dependency download on a new checkout
./scripts/verify.sh                     # lint, Rust/IPC tests, precision, resampling
./scripts/build-macos.sh                # release build and ad hoc signed XPC bundle
python3 scripts/test-xpc-integration.py
open -n -W dist/TrueRenderer.app --args --sampling-smoke
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-sampling-screenshots
```

Native tests need a normal macOS desktop session, because an extra terminal sandbox can block Core Image, Cocoa, or XPC. `verify.sh` explicitly runs the ignored tests after building the worker.

The external-format fixture generator additionally requires `cwebp`, used only to produce the WebP test image:

```sh
python3 scripts/generate-format-fixtures.py
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-formats
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-cache
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-previews
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --preview-performance-smoke
python3 scripts/generate-preview-corpus.py
./dist/TrueRenderer.app/Contents/MacOS/TrueRenderer --verify-preview-navigation
python3 scripts/test-preview-memory.py --bundle dist/TrueRenderer.app --mode cache
python3 scripts/test-preview-memory.py --bundle dist/TrueRenderer.app --mode viewer
open -n -W dist/TrueRenderer.app --args --formats-smoke
open -n -W dist/TrueRenderer.app --args --settings-smoke
python3 scripts/test-installed-macos.py  # full native suite, Finder, independent runtime copy
python3 scripts/test-preview-lifecycle.py --runs 20
python3 scripts/preview-environment.py
```

Fixtures are generated locally from formulas, so no photographs are downloaded. The navigation generator creates 1,000 distinct 512×384 Bayer DNGs, taking roughly 400 MB for sources plus their disposable cache under `var/`; this is a scheduler/cache workload, not the required real-camera 12/24/45 MP qualification. Cache tests cover exact fp32 round trips, invalidation, corruption, cancellation, quotas, expiration, LRU eviction, abandoned writes, locks, and link handling. The native cache check measures cold and warm loads for generated JPEG, DNG, and PNG files and confirms that warm loads require no decoder jobs. Scope and actual results are recorded in [reports/VERIFICA.md](reports/VERIFICA.md).

## Project documents and local data

| Path | Purpose |
|---|---|
| [PLAN.md](PLAN.md) | Operational R0–R4 plan, completed work, and remaining tasks |
| [docs/TrueRenderer-Architettura.md](docs/TrueRenderer-Architettura.md) | Architecture and full comparison against the initial requirements |
| [docs/avanzamento.md](docs/avanzamento.md) | Progress register |
| [docs/adr/](docs/adr/) | Decisions and recorded boundaries for each increment |
| `var/settings.json` | Persistent cache preferences |
| `var/library.sqlite`, `var/backups/` | Durable annotations and backups; never part of cache cleanup |
| `var/index.sqlite` | Rebuildable catalog index |
| `<image folder>/.truerenderer-cache/` | Disposable rendered-image cache and temporary files |
| [reports/](reports/) | Verification results, generated-image screenshots, dependency inventory |
| `target/`, `.tools/` | Build artifacts and local development tools, not image caches |

The workspace contains `tr-core`, `tr-app`, `tr-store`, `tr-platform`, `tr-worker`, `tr-render`, and `apps/desktop`. Third-party dependencies keep their own licenses; see [NOTICE.md](NOTICE.md) and the [inventory](reports/dependency-inventory.json). Development records are kept in Italian.

## Status

This is a development prototype, not a qualified 1.0. Remaining work includes the full R0 gates, integrated memory qualification, device-loss recovery, live source-revision monitoring, faster XPC recovery, Windows support, ICC and display qualification, accessibility, complete catalog and XMP workflows, a real RAW and LibRaw camera matrix, tiles and gigapixel images, installers, and notarization. Worker memory is supervised rather than hard-capped, and XPC recovery can take roughly ten seconds.

LibRaw is planned for the future cross-platform RAW pipeline and is not bundled in 0.1.5. Its open-source licensing allows use in a commercial proprietary product without buying a commercial license, subject to the selected CDDL/LGPL obligations. See the [official licensing statement](https://www.libraw.org/about) and the project's [licensing review](docs/licenza-libraw.md).

## Trademarks

TrueRenderer is an independent project, not affiliated with or endorsed by any company named here. Apple, macOS, Metal, Core Image, ImageIO, and ColorSync are trademarks of Apple Inc. Adobe and DNG are trademarks of Adobe Inc. Camera and format names such as Canon CR3, Nikon NEF, Sony ARW, and Fujifilm RAF identify formats owned by their respective manufacturers. All other trademarks belong to their owners and are used only to describe compatibility. The full notice is in [NOTICE.md](NOTICE.md).
