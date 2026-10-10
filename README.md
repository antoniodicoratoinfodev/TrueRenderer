# TrueRenderer

### Your photographs. Your vision. Every detail under your control.

**TrueRenderer brings browsing, RAW development, precise editing and side-by-side comparison into one native desktop workspace.** Move from a folder of photographs to a carefully finished image, with responsive adjustments, powerful colour tools and originals that always remain untouched.

![A mountain lake photograph in TrueRenderer, with the Develop panel and filmstrip](reports/readme-2026-10-06/develop.jpg)

**GPU-accelerated editing · Five RAW engines on macOS · Reversible adjustments · Local-first library**

## Stay in the creative flow

A good editing session keeps your attention on the photograph. TrueRenderer combines GPU acceleration for light and colour adjustments with parallel CPU processing, so your hardware contributes directly to the editing experience.

**Standard and Full previews** let you choose the balance between speed and detail, globally or for an individual photograph. During a slider gesture, an adaptive preview follows your adjustments; after you stop, it refines automatically. Background preparation, nearby-image preloading and reusable previews help keep navigation moving.

Tune performance to your machine with CPU, GPU and memory preferences. Keep the controls close, move through the filmstrip and refine the image without leaving your workspace.

## Shape light. Refine colour. Make it yours.

From a subtle tonal correction to a distinctive creative treatment, the Develop panel gives you direct control over the photograph.

| Your intention | Your tools |
|---|---|
| Balance the light | Exposure, brightness, contrast, highlights, shadows, whites and blacks |
| Sculpt the tones | Point curves and individual RGB channel curves |
| Find the right colour | RAW white balance, RGB warmth and tint, saturation, vibrance and warm-tone protection |
| Build a distinctive look | Eight-band HSL mixer, shadow/midtone/highlight colour grading and black-and-white controls |
| Refine texture | Texture, clarity, dehaze, sharpening and luminance/chroma noise reduction |
| Strengthen the composition | Crop, aspect ratios, rotation, flips, straightening and perspective controls |
| Correct the optics | Manual distortion, vignette, chromatic aberration and defringe adjustments |
| Guide attention locally | Brush, radial and linear gradients, luminance and hue masks, with feathering and inversion |
| Adjust a chosen colour | Sampled colour ranges, hue/chroma/luminance corrections and separate uniformity controls |
| Organize local adjustments | Named photographic layers, empty layers, opacity, fill, visibility, duplication, ordering and editable combined masks |

![TrueRenderer's creative colour controls beside a mountain photograph](reports/readme-2026-10-06/colour.jpg)

Click in the tone-curve graph to add anchor points, then drag them to shape the tones. Select a point to refine its Input and Output with sliders or numeric values; use **Delete point** to remove it. Neighbouring anchors stay in place, and the midtone slider preserves your custom points. The black and white endpoints can also move horizontally and vertically to set input and output levels.

Double-click a photographic slider to restore its default. The **Reset all** button beside Undo/Redo restores all adjustments, including as-shot RAW white balance, and can itself be undone. Double-clicking either Apple RAW WB slider restores the complete as-shot white balance.

Open **Crop…** from the Develop actions or the photograph’s right-click menu. Choose a ratio in the compact panel, swap its orientation, then drag the edges or corners to resize. Choose **Move the frame** or **Move the photo** for the interior gesture; the optional thirds grid helps composition. **Full image** resets only the crop. **Apply crop** saves one reversible change; **Cancel crop** or Escape restores the starting crop. Reopening the tool shows the full available image again, including previously excluded areas. Space-drag pans the view independently. With the photo focused, arrows move, +/− resize, X swaps orientation and Enter applies; Shift gives a larger step. Right-click or Shift+F10 opens crop commands, with **Photo menu** giving access to the usual photographic actions. A crop draft stays open when you navigate elsewhere and must be confirmed or cancelled before closing the app.

In **Develop → Tools**, choose **On a new layer** or **On the selected layer**, then search for sampled colour, local light, RGB point curves, tonal levels, the colour mixer or the channel mixer. **Sample on photo** temporarily shows the input to that layer; use the final rendering for a native sample. Adjust the colour or its uniformity, then add, subtract or intersect brush, gradient, luminance and colour masks. In **Develop → Layers**, **New empty layer** creates a layer you can populate later with **Apply a tool to this layer**. Every layer has separate **Opacity** and **Fill** controls; with the current Normal blend, their product attenuates the effect independently of the mask overlay. Tool icons and accents follow tone, curves, colour and toning; RGB/CMYK indicators and warm/cool or green/magenta slider guides help identify controls. Labels and selection states remain visible independently of colour. The stack runs from bottom to top and its masks follow the source through crop and rotation. These photographic layers use CPU processing; image compositing and assisted selections are separate planned extensions.

The same selector includes **Four-wheel grading**, **Black and white mixer**, **Color filter** and **Gradient map**. Grade shadows, midtones, highlights and the whole image with hue/chroma wheels, zone luminance, balance and overlap. The wheels and sliders share their values; arrows make fine changes, Shift increases the step and Home resets hue/chroma. Use **Convert to B&W** and the eight family sliders to shape a monochrome image, then add grading for a split tone. Choose a filter colour and density, or build a gradient palette with editable stops, amount and reversal. Each tool starts neutral and supports the layer’s mask, intensity and undo history.

**Selective color** adjusts C/M/Y/K components across six colour families plus whites, neutrals and blacks. Choose **Relative** for a percentage of the RGB signal or **Absolute** for a linear offset; **Reset family** affects only the selected family. This is a creative RGB adjustment. **Colorize** adds a common hue to colour or monochrome images, with separate amount, chroma and brightness. **Tonal controls** adds local exposure, brightness, contrast, shadows, highlights, whites and blacks, with an adjustable pivot under **Tonal transitions**. **Exposure, offset and gamma** provides the three technical operations in that order, including signed gamma for negative values. All four are available from **Develop → Layers** and start neutral.

In **Tonal levels**, choose **Analyze input** to load the native adjustment input and its histogram, then **Auto composite** or **Auto independent channels**. Auto freezes the 1st/99th-percentile black and white points; independent channels may change colour balance. Analysis covers the entire developed crop, before the layer mask and intensity. **Pick black**, **Pick gray** and **Pick white** work on the selected composite/channel; gray on RGB aligns the three channels while retaining their black/white points. **Reset channel** affects only the selected channel. **Finish analysis** or Esc restores the full preview without applying a new analysis. Results and their source/region references survive reopening and undo; recalculation is explicit.

**Luminance curve** provides a point editor on linear Y, preserving channel differences. **Parametric curves** offers four tonal zones and three adjustable boundaries, with a choice of luminance or RGB mapping and a live curve graph. Both tools start neutral and use the layer's existing mask and intensity.

Experiment freely. **Undo, Redo, Reset all and Before/After** keep decisions reversible, while saved editing recipes preserve your work in the local library. Interrupted recipe saves can recover a draft with explicit Retry or Discard. Copy selected light, tone-curve and RGB colour adjustments between photographs to build a consistent look; that transfer does not include the new layer stack.

Use **Develop → Saved looks** to save selected photographic layers in your library and reuse them on another photo; in compact windows, open **Actions → Saved looks**. Select the destination in Preview, choose the layers, intensity and whether to include saved masks, then **Try on photo** to compare with and without the look. **Apply look** appends independent editable layers; **Cancel preview** or Esc leaves the photo unchanged. RAW settings, base adjustments and crop stay with the destination. Saved Auto values remain fixed. Looks can be renamed, archived and restored, and are included in library backups.

## Choose the RAW rendering that suits the photograph

The RAW engine is part of the creative decision. On macOS, choose **Apple RAW, LibRaw bilinear, LibRaw AHD, TrueRenderer fp32 or trueRendererExperimental** for supported inputs. The active engine stays visible in the toolbar, and native white-balance controls work with the selected engine.

**trueRendererExperimental** has an independent DNG path without LibRaw calls: Bayer integer DNG, uncompressed or supported lossless JPEG, with one- or two-illuminant matrix profiles. Unsupported calibration features are rejected. Direct NEF/RAF decoding and measured camera colour qualification remain open; the other engines and platform defaults retain their existing behaviour. See the [Experimental contract](docs/progetto-truerenderer-experimental.md).

Start from as-shot white balance, adjust it manually, estimate it automatically or sample a neutral area. Apple uses native RAW temperature/tint; the other engines adjust red/blue sensor gains relative to as-shot before demosaicing. Experimental resolves its DNG colour matrix with the selected white balance. A floating-point processing pipeline provides room for demanding tonal and colour adjustments before the final output conversion.

TrueRenderer fp32 neutralizes sensor-clipped highlights to prevent false magenta skies and reflections. It preserves unclipped channels and floating-point headroom; colour and detail lost in a fully saturated sensor cannot be recovered.

## See the differences that matter

Compare photographs side by side with synchronized navigation. Judge composition, exposure, colour and fine detail in context, then move to **physical 1:1** for a closer inspection.

Right-click a photograph in the grid, filmstrip, viewer, inspector preview or Explorer to assign it to **A** or **B**, or choose **Compare Before/After** for its initial development and current adjustments. A/B assignments stay in place across folders for the current session. The same menu provides ratings, colour labels, keywords, supported adjustment transfer, geometry, export, final-output preview, file reveal and path copying. In the grid and filmstrip, group commands use the selection only when it includes the clicked photo; the menu shows the target count. Viewer commands refer to the clicked pane. Shift+F10 opens the menu for a focused photograph.

![Two Grand Teton photographs side by side in TrueRenderer](reports/readme-2026-10-06/compare.jpg)

Histograms, a pixel sampler and camera metadata put useful evidence alongside the image. Check shutter speed, aperture, ISO, focal length and lens information as you decide which frame deserves the final edit.

## Bring order to your photographs

Explore your folders, scan a thumbnail grid or stay immersed in the viewer. Search, favourites and navigation history help you return to the photographs you need.

![TrueRenderer's photographic grid with folder navigation, histogram and shooting metadata](reports/readme-2026-10-06/library.jpg)

Use **star ratings, colour labels, reject flags and keywords** to turn a folder into a considered selection. Switch between Library and Explorer, filter your collection and keep the information panel at hand.

Your originals are read-only. Your library, annotations, editing recipes and backups stay on your computer. **No account, photo upload or cloud subscription is required.**

## Finish with the output you need

Export your edited photographs as **JPEG, PNG or TIFF**, with 8-bit or 16-bit options where supported and floating-point TIFF for workflows that need extended numerical values. Export multiple photographs and set a target long edge when preparing a delivery.

Exports apply the saved photographic recipe to the native source. The final-rendering view lets you inspect the PNG/TIFF output rendering before writing your files.

For scientific imagery, TrueRenderer also opens **2D FITS images**, with linear or asinh display, a histogram and native-value inspection.

## Get started

Try the four public-domain landscape photographs shown here from **Library → Sample photos**, beside the test corpus. Their original files and credits are included in [`sample-photos/`](sample-photos/manifest.json); the macOS bundle includes them for offline use.

Open an image or a folder, choose a photograph and switch to **Develop**. Use **Settings → Previews and RAW** to choose preview quality and a RAW engine. English and Italian interfaces are available, with 100%, 150% and 200% UI scaling.

<details>
<summary><strong>Build and run from source</strong></summary>

Run these commands from the repository root. The repository pins Rust 1.98.1; Python 3 is required by the supporting scripts.

**macOS** — Install Rust 1.98.1 and the Xcode Command Line Tools, then:

```sh
./scripts/cargo-local.sh fetch --locked
python3 scripts/generate-format-fixtures.py
./scripts/build-macos.sh
open dist/TrueRenderer.app --args --open "/path/to/photos"
```

Use the macOS app bundle for external photographs: it includes the isolated decoder services.

**Windows** — Install Rust 1.98.1 with rustfmt and Clippy, Visual Studio Build Tools with the Desktop development with C++ workload and Windows SDK, Python 3, and Git for Windows. In PowerShell:

```powershell
.\scripts\dev-windows.ps1 fetch --locked
.\scripts\dev-windows.ps1 verify --gui
.\target\debug\truerenderer.exe
```

The [documentation index](docs/README.md) covers platform-specific format support, architecture and verification. Apple RAW is macOS-specific; camera and file compatibility depend on the selected decoder and platform.

Keep `var/library.sqlite` and `var/backups/`: they contain durable library data. To remove compilation artifacts, use `./scripts/cargo-local.sh clean`.

</details>

---

TrueRenderer is proprietary software. See the [license](LICENSE) and [third-party notices](NOTICE.md).

Screenshots show the English interface at **100% UI scale**. Landscape photographs: National Park Service, including NPS / David Restivo; public-domain source credits and capture details are recorded [here](reports/readme-showcase-macos-2026-10-06.json).
