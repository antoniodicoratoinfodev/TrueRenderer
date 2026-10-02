# TrueRenderer

## See the image. Understand the rendering.

TrueRenderer is a native desktop application for photographers, astrophotographers, retouchers, archivists, imaging specialists, astronomers, researchers and scientists who need to browse, inspect and compare photographs and other still images through a rendering path they can understand.

It brings each image, its technical context and the choices behind its appearance into one focused workspace. Originals remain untouched, the library stays on your computer, and no account, upload or subscription is required.

![TrueRenderer viewer with the Develop controls, edited photograph and filmstrip](reports/readme-viewer.png)

*Current macOS development build with the Develop tab, an edited image and filmstrip. All screenshots use 100% UI scale and the generated test corpus. [Capture details and verification](reports/readme-screenshots-macos.json).*

## A clearer way to look

### Browse naturally

Open an image or a folder, then move between the thumbnail grid, filmstrip and focused viewer. Library and Explorer views, search, favourites and navigation history keep large collections easy to explore.

Folder preparation runs in the background with visible progress and controls to pause, resume or cancel.

### Inspect real detail

Move from Fit view to physical 1:1 when you need to judge focus, texture, noise or retouching without an accidental resize getting in the way. Metadata, pixel values, histogram and rendering information remain close at hand.

The photographic Inspector has two tabs: **Information** opens by default in the grid, and **Develop** in the viewer. Each view keeps its tab choice for the session. The filename and tabs stay visible while the contents scroll.

**Information → Shooting data**, below **File**, shows camera, lens, shutter speed, aperture, ISO and focal length from readable standard EXIF tags. Missing fields appear as “—”; when no shooting data is available, a single message replaces the empty list. Hover over a value for its source. This read-only profile covers JPEG, PNG and classic TIFF-based files, including supported RAW containers; proprietary lens names and BigTIFF metadata are not inferred.

### Compare with intent

Place two images side by side with synchronized navigation to compare focus, exposure, RAW development or near-duplicate frames.

### Choose how RAW is interpreted

RAW development always involves interpretation. TrueRenderer makes that choice explicit with Apple RAW on macOS, LibRaw bilinear, LibRaw AHD and the experimental TrueRenderer fp32 engine for supported cameras.

### Develop a photograph

The **Develop** tab has reversible exposure, brightness, contrast,
highlights, shadows, whites, blacks, a basic tone curve, relative RGB warmth/tint
and saturation. **Vibrance** selectively adjusts less saturated colours; optional
**Protect warm tones** reduces its effect on warm hues without identifying skin.
Using these controls opts that revision into photographic process 2; existing
recipes retain their previous rendering.

#### History and reusable adjustments

Edits are saved as versioned recipes in the local library;
**Undo**, **Redo** and **Before/After** leave the original file untouched.
Changing photos saves the current draft. If saving fails, **Retry save** keeps
your adjustments; **Discard draft and reload saved recipe** explicitly abandons
the unsaved changes and reloads the library version. Retrying a failed history
operation reloads the saved recipe without creating an extra revision.

**Reset light**, **Reset curve** and **Reset colour** clear only their group of
adjustments, preserving the RAW engine and native white balance. Each reset is
saved and can be undone or redone; Reset colour also clears vibrance and warm-tone
protection.

**Copy and paste adjustments** keeps a session copy of the saved recipe. Select
another photograph, choose Light, Tone curve and/or RGB color, then **Paste selected
groups**. Pasting replaces only those groups and creates one undoable revision;
the destination keeps its RAW engine and native white balance. The copy is cleared
when the app closes; pasted edits remain in the library. With image focus,
use **Alt+Shift+C/V** to copy/paste the selected groups, and
**Cmd/Ctrl+Alt+Z** / **Cmd/Ctrl+Alt+Shift+Z** to undo/redo photographic edits.
**Cmd/Ctrl+Z** continues to undo annotations. Editing shortcuts wait until the
recipe is loaded and saved, and stay inactive in text fields, menus, settings and
the photo export window.

#### RAW white balance

For a decoded RAW, the panel offers as-shot/reset and native controls: Apple RAW
uses temperature/tint; LibRaw bilinear, AHD and TrueRenderer use red/blue sensor
gains relative to as-shot, before demosaicing. These gains are not Kelvin. The
values are stored with the photograph's engine.

Use **Cancel RAW WB analysis** to interrupt a running estimate without changing the recipe or its history.

**Auto RAW WB** estimates native WB parameters from the unedited RAW render,
assuming average neutral scene colour. **RAW WB from 5×5 area** uses the last
sampled native pixel as the centre of a neutral patch. Both work through the
isolated decoder on supported RAWs up to 48 MP, save resolved values and support
Undo. Analysis may take multiple RAW developments (up to 120 seconds); unstable,
out-of-range or insufficient samples are refused without changing the recipe.
These controls are separate from Auto RGB and do not identify the scene illuminant.

Apple RAW also offers adjustable Tungsten (3200 K), Daylight (5500 K), Cloudy
(6500 K) and Shade (7500 K) presets, all with tint zero, plus As-shot. These are
starting points, not measurements of scene lighting. Presets save numerical WB
values and support Undo; changing the sliders shows Custom when appropriate.

#### Automatic adjustments and sampling

**Auto exposure** suggests a conservative exposure; **Auto RGB · grey world**
assumes an average neutral scene and adjusts the relative RGB controls. Choose
**Load native for Auto** when needed. Both actions save their resolved values and
support Undo; Auto RGB requires zero saturation and vibrance and does not change native RAW WB.

To neutralize a colour cast, choose **Verify final rendering**, then
**Return to extended fp32 render**. Hover over an opaque, unclipped pixel that
should be neutral and use **Neutralize RGB sample**. This adjusts relative RGB
warmth/tint; it does not change the camera's RAW white balance.
Choose **Picker area**: 1×1, 5×5 (default) or 11×11 native pixels. Area sampling
reduces the influence of isolated outliers and rejects mixed-colour regions,
incomplete areas at image edges and areas with too few usable pixels. The panel
shows valid pixels and chromatic spread. The picker requires zero saturation
in the edit recipe.

#### Check the final rendering

The quick edited view is provisional. **Verify final rendering** builds an
**sRGB16 export preview** from native resolution when the memory budget permits
it. It simulates native-size PNG/TIFF16 conversion before reducing the image,
so fitted views follow the exported file. Edits retain extended fp32 working
values. Choose **Return to extended fp32 render** for working-space inspection
and the RGB picker. JPEG compression and resized exports need separate checks.
Grid and filmstrip thumbnails reflect saved or in-progress edits. The inspector
histogram follows its edited thumbnail preview and labels the preview level.

In short windows, copy/paste and final-render checks are in **Develop → Actions**.

The remaining tools in the [photographic development plan](STATO.md#piano-sviluppo-fotografico),
including optics, spatial detail filters, masks,
retouching and presets, are still in development.

### Keep the work yours

Ratings, rejection flags, colour labels and keywords live in a local library, separate from the originals, with backups and JSON export.

### Export photographs

Select photos and choose **Menu → Export photos**. Export JPEG with adjustable
quality, PNG 8/16-bit with lossless compression choices, TIFF 16-bit or linear
float32, and two distinct DNG operations: developed linear RGB or sensor mosaic.
Choose a destination and optional long-edge size; existing files are never replaced.
Exports use native development, independent of thumbnail quality.
JPEG, PNG and TIFF exports apply each photo's saved edit recipe. The recipe and
RAW engine are frozen when a batch starts. Linear DNG retains technical RGB
development, while RAW DNG retains the mosaic; neither bakes in creative edits.

PNG/TIFF 16-bit offer more output levels than JPEG but still clip to their output
range. TIFF float32 preserves extended linear working values. Linear DNG is
16-bit developed RGB. Reopen exported DNG files with LibRaw bilinear/AHD, not
Apple RAW or the TrueRenderer mosaic engine. RAW DNG is limited to the active-area mosaic of
Nikon D750/D40; it excludes optical margins and private camera metadata.
**Keep the original NEF: neither DNG operation is an archival copy.**

### Inspect scientific data

Read-only FITS supports the first eligible 2D primary/IMAGE plane with BITPIX
8, 16, 32 or −32, scaling, units and missing/nonfinite samples. The inspector
provides a native-value sampler, full-plane histogram and linear/asinh display
stretch. Cubes, compressed FITS, 64-bit samples, WCS and FITS export are excluded.
The scientific resampling path currently uses CPU, even in GPU mode.

Performance preferences also offer optional SDR10 and SDR16-float presentation
after restart, with explicit fallback to compatible SDR8. This removes an 8-bit
presentation bottleneck where supported; it does not enable HDR or certify the
physical display's bit depth. Working data and caches remain fp32.

See the [format and precision contract](docs/esportazione-precisione-fits.md)
and [verified scope](STATO.md) before choosing a workflow.

## Rendering you can explain

TrueRenderer shows the source, decoder, RAW recipe, working representation and preview resolution instead of hiding them behind a generic preview.

Its processing path uses extended linear Rec.2020 and 32-bit floating point precision. Physical 1:1 preserves aligned source samples, while reduction and enlargement use documented filters.

RAW files do not have one universally correct appearance, and display colour depends on the complete system. TrueRenderer makes those boundaries visible, so the image on screen is easier to understand and evaluate.

## Designed around the photograph

The interface keeps the image at the centre. Explorer stays compact, technical information lives in collapsible sections, and familiar controls remain available across the grid, viewer and comparison workspace. Develop pairs each full-width slider with an editable value, while edit indicators sit below thumbnails to keep photographs unobstructed. Neutral selection borders and consistent navigation icons keep the controls easy to distinguish.

![Thumbnail grid with Library, shooting data and a saved edit indicator](reports/readme-grid.png)

In smaller windows, navigation and the Inspector open as floating panels. The compact Inspector uses more of the available height; the image preview and histogram can be expanded when needed. Portrait previews fit within a bounded height so File metadata stays close at hand.

![Develop inspector at 100% UI scale, with the histogram and Light controls visible](reports/readme-inspector.png)

*Develop at 100% UI scale. [Compact navigation](reports/readme-compact.png).*

![Performance preferences with SDR presentation precision and processing options](reports/readme-preferences.png)

Preferences are grouped by purpose, and the interface is available in English and Italian.

Interface size can be set to 100%, 150% or 200% in **Settings → General → View · this session**. Cmd/Ctrl +/−/0 controls the photograph only. In short windows the filmstrip hides automatically to keep space for the viewer.

## Local by design

Original files are read-only. Previews use a separate cache, while ratings, keywords and backups remain durable library data.

In **Settings → Cache and data**, one space limit applies either per folder or across all folders known to the current library. The total limit is optional. You can disable unused-preview expiry or enable periodic cleanup of folders that are not open; periodic cleanup runs every minute while the app is running and uses the same quota and expiry. **Clean known caches now** applies the saved policy, while **Clear folder cache** clears only the current folder. The summary reports the last cleanup measurement and incomplete checks. Previously visited folders become known when reopened with this version; disconnected folders are retried. No cleanup service runs after the app closes.

Cache and memory limits pair an editable value with a full-width slider. Sizes use decimal B/kB/MB/GB; MB fields also accept a decimal comma.

![Cache preferences with aligned quota controls, expiry and optional cleanup across known folders](reports/readme-cache.png)

External decoding is isolated in sandboxed XPC services on macOS and a confined worker on Windows. Decoder failures remain separated from the library writer.

TrueRenderer targets Apple silicon Macs and x86-64 Windows PCs. JPEG, PNG and TIFF use its portable path; native macOS services and a growing set of camera profiles extend format support.

On Windows, the portable decoder applies embedded RGB matrix/TRC ICC v2/v4
profiles through Little CMS, including the profiles in the app's JPEG and TIFF
exports. Linear TIFF float preserves extended values and transparency. LUT,
Gray/CMYK and conflicting or malformed profiles remain unsupported and produce
an explicit error; this is not universal ICC compatibility. PNG exports also use
the supported sRGB path.

## Availability

TrueRenderer is currently a development preview available from source. Follow the verified scope, open work and latest test results in [STATO.md](STATO.md).

## Build and run

The repository pins Rust 1.98.1 and uses Python 3 for fixtures and verification. Run commands from the repository root.

### macOS

With the Xcode Command Line Tools installed:

```sh
./scripts/cargo-local.sh fetch --locked
python3 scripts/generate-format-fixtures.py
./scripts/verify.sh
./scripts/build-macos.sh
python3 scripts/test-xpc-integration.py
open -n dist/TrueRenderer.app --args --open "/path/to/image.jpg"
```

### Windows

Install Rust 1.98.1 (with rustfmt and Clippy), Visual Studio Build Tools with the
Desktop development with C++ workload and Windows SDK, Python 3, and Git for Windows.
The PowerShell helper uses Git Bash and the same local-toolchain wrapper as macOS;
Rust installed under `.tools/cargo` and `.tools/rustup` takes precedence over the system installation.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/dev-windows.ps1 fetch --locked
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/dev-windows.ps1 verify --gui
.\target\debug\truerenderer.exe --open "C:\Photos\image.jpg"
```

`verify` builds the workspace and runs formatting, Clippy, tests, worker protocol
and numerical checks. `--gui` also opens the native app for an automated surface
test. Each run saves reports and generated screenshots under a new `var/verify-*`
directory, using a separate test library. Private camera tests require `TR_RAW_SAMPLE`.
From Git Bash, use `./scripts/cargo-local.sh` and `./scripts/verify.sh` directly.
The execution policy option applies only to that PowerShell process; it does not
change the computer's policy.

## Documentation

The [documentation index](docs/README.md) links the specifications, architecture decisions and verification reports. TrueRenderer is proprietary software; see [LICENSE](LICENSE) and [third-party notices](NOTICE.md).
