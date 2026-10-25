<p align="center">
  <img src="assets/npnp.png" alt="npnp icon" width="260" />
</p>

<h1 align="center">npnp (Normalize Pin Net Pad)</h1>

<p align="center">
  <strong>A command-line tool for batch-exporting components from LCSC for Altium Designer (AD), KiCad and Cadence (OrCAD Capture / Allegro) to use.</strong>
</p>

<p align="center">
  <a href=".github/workflows/release.yml"><img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-2ea44f?style=flat-square" alt="platform Windows macOS Linux"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-edition%202024-f28d1a?style=flat-square" alt="rust edition 2024"></a>
</p>

<p align="center"><a href="README.en.md">English</a> | <a href="README.md">简体中文</a></p>

---

## Before You Start

> [!NOTE]
> The source code is public again, but please follow the [LICENSE](LICENSE) when using this project.

`npnp` stands for `Normalize Pin Net Pad`. It is the CLI tool I built while reverse-engineering parts of the `Altium Designer` library format and cleaning up the LCSC / EasyEDA export workflow. It is intended for users who need scripting, batch processing, CI, or simply prefer a command-line tool. If you need a GUI, use the related [`SeEx`](https://github.com/ref42/seex) application.

`npnp` and `SeEx` should keep the same export behavior wherever possible, especially:

- KiCad `3D` export writes downloaded STEP data directly, without the expensive STEP parse-and-rewrite pass.
- KiCad STEP downloads use the same LCSC model endpoint and HTTP transport as Altium.
- Both Altium and KiCad support Chinese or English metadata export; when the requested language is missing, existing source metadata is preserved as fallback.
- Generated libraries are intended for real EDA use, but they should still be visually checked before production.

If you find an issue in exported symbols, footprints, 3D models, or metadata, please open an `issue` with the LCSC ID, target EDA, export command, and screenshots. PRs are not merged by default unless the maintenance direction is agreed in advance.

## What Can npnp Do?

### KiCad

- [x] Export KiCad-compatible `Symbol`, `Footprint`, and `3D` model formats.
- [x] Export individual components or batch export from a text file.
- [x] Append or update existing `.kicad_sym`, `.pretty`, and `.3dshapes` libraries.
- [x] Write STEP models directly into `.3dshapes` and reference them from footprints.

### Altium Designer (AD)

- [x] Export individual `.SchLib` and `.PcbLib` libraries. `.PcbLib` embeds upstream STEP data when available.
- [x] Support batch export.
- [x] Export multiple components into one merged `.SchLib` / `.PcbLib` pair.
- [x] Append new components to an existing npnp merged library without replacing the old components.

### Cadence (OrCAD Capture / Allegro)

- [x] Export `Capture` schematic symbols as one `Library XML` file, which `Capture` converts to an `.olb` with `File > Import > Library XML`; multi-part EasyEDA symbols become sections of a single Capture part.
- [x] Export `Allegro` footprints as one `SKILL` script per footprint plus one library script; `skill load("<library>_allegro/<library>.il")` in the `Allegro PCB Editor` command window builds the `.dra`, `.psm` and `.pad` files in that folder.
- [x] Export `3D` models (`step`), mapped into the footprint at the position EasyEDA uses.
- [x] Standard packages (chip resistors and capacitors, `SOT`, `SOD`, `SOIC`, `QFN`, ...) get `IPC-7352` lands checked against the manufacturer's; other packages keep the manufacturer's lands, with silkscreen, assembly layer and place bound generated per `IPC-7352`.
- [x] `npnp` runs no Cadence program itself; the import is done inside `Cadence`.

### Shared Features

- [x] Export metadata in Chinese or English.
- [x] Use parallel download and batch processing.
- [x] Export `3D` models independently.
- [x] Remove the EasyEDA logo (watermark) from downloaded `3D` models by default and rebuild them; a model the logo cannot be removed from safely is exported as it is and listed in the export result. `--keep-model-logo` keeps the original models.
- [x] Export raw symbol / footprint JSON for troubleshooting.
- [x] Provide multi-platform Windows, macOS, and Linux release packages.

## Supported EDA Versions

> [!NOTE]
> These are the versions currently known to work. npnp may support additional versions. If a library exported by npnp cannot be opened by an older EDA version, please open an issue so compatibility can be considered.

- [x] `KiCad 9.0` to `Latest`
- [x] `Altium Designer 23.x` to `Latest`
- [x] `Cadence SPB 26.1` (OrCAD X Capture / Allegro X); the `Allegro` footprint scripts need a release that supports `SKILL` (`Allegro PCB Designer` or `OrCAD PCB Designer Professional`)

## What Is Planned?

- [ ] Continue improving the current Altium / KiCad workflows and fix more hidden `bugs`.
- [ ] Improve solder mask handling for irregular pads during `.PcbLib` export.
- [ ] Support more `EDA` tools, such as `PADS`.
- [ ] Add more regression fixtures for unusual symbols, footprints, and `3D` models.
- [ ] Improve documentation for batch merge, append, and CI workflows.

## Downloads

Each tagged release builds native `npnp` CLI archives for:

- Windows x86_64: `npnp-vX.Y.Z-windows-x86_64.zip`
- Windows ARM64: `npnp-vX.Y.Z-windows-aarch64.zip`
- Linux x86_64: `npnp-vX.Y.Z-linux-x86_64.tar.gz`
- Linux ARM64: `npnp-vX.Y.Z-linux-aarch64.tar.gz`
- Fedora x86_64: `npnp-vX.Y.Z-fedora-x86_64.tar.gz`
- Fedora ARM64: `npnp-vX.Y.Z-fedora-aarch64.tar.gz`
- Arch Linux x86_64: `npnp-vX.Y.Z-arch-x86_64.tar.gz`
- Arch Linux ARM64: `npnp-vX.Y.Z-arch-aarch64.tar.gz`
- macOS Intel: `npnp-vX.Y.Z-macos-x86_64.tar.gz`
- macOS Apple Silicon: `npnp-vX.Y.Z-macos-aarch64.tar.gz`

## CLI

Type `npnp p` to print ready-to-run commands. Export commands are grouped by target EDA tool:

```bash
npnp search C2040 --limit 5

npnp altium C2040 -o altium_libs
npnp altium -i ids.txt -o altium_libs --name MyParts -j 42
npnp altium -i new_ids.txt -o altium_libs --name MyParts --append -j 42

npnp kicad C2040 -o kicad_libs --name MyParts
npnp kicad -i ids.txt -o kicad_libs --name MyParts -j 42
npnp kicad -i new_ids.txt -o kicad_libs --name MyParts --append -j 42

npnp cadence C2040 -o cadence_libs --name MyParts
npnp cadence -i ids.txt -o cadence_libs --name MyParts -j 42
npnp cadence -i new_ids.txt -o cadence_libs --name MyParts --append -j 42

npnp model C2040 -o step_models
npnp model -i ids.txt -o step_models -j 42
```

Useful shared flags:

- `--full`: export all supported library targets for that EDA tool.
- `--force`: replace existing generated outputs.
- `--language auto|en|zh`: select metadata language. `auto` uses source metadata, while `en` fetches English metadata with source fallback.
- `-j`: maximum number of concurrent exports when using `-i`.
- `-i`: reads IDs from any plain-text file; the filename does not need a `.txt` extension.
- `--name`: optional override for the generated library base name.
- `--append` updates the library collection inside the selected `-o` directory. For Altium it discovers the merged `.SchLib`/`.PcbLib`; for KiCad it discovers the `.kicad_sym`, `.pretty`, and `.3dshapes` libraries; for Cadence it adds to the existing `<library>.xml` and `_allegro` folder.
- `--keep-model-logo`: keep the EasyEDA logo on downloaded `3D` models (it is removed by default).

### Export contract

- Generated component and model names are deterministic: `<source-name>__<LCSC-ID>`. The ID is also retained in the schematic metadata as `NPNP_COMPONENT_ID`.
- Missing STEP data does not prevent symbol or footprint export; the console summary identifies missing-model counts where the target supports 3D output.
- The Cadence target writes `<library>.xml` (Capture Library XML), `<library>_allegro/` (one `SKILL` script per footprint plus the library script) and `<library>_step/` (the `3D` models). `npnp` calls no Cadence program: the `.olb` comes from `File > Import > Library XML` in Capture, and the `.dra`/`.psm`/`.pad` files from `skill load("<folder>/<library>.il")` in the Allegro command window.
- Fresh merged output refuses to overwrite an existing library. Use `--append` to preserve the existing library collection and add new components, or `--force` when replacing standalone generated files is intentional.

## Export Examples

<table>
  <summary>KiCad examples</summary>
  <tr>
    <td align="center"><img src="imgs/kicad_symbol.png" alt="KiCad symbol" width="280" /><br /><sub>Symbol library</sub></td>
    <td align="center"><img src="imgs/kicad_footprint.png" alt="KiCad footprint" width="280" /><br /><sub>Footprint library</sub></td>
    <td align="center"><img src="imgs/kicad_3dmodel.png" alt="KiCad 3D model" width="280" /><br /><sub>3D model</sub></td>
  </tr>
</table>

<table>
  <summary>Altium PCB library examples</summary>
  <tr>
    <td><img src="imgs/pcb_01.png" alt="Altium PCB example 1" width="420" /></td>
    <td><img src="imgs/pcb_02.png" alt="Altium PCB example 2" width="420" /></td>
    <td><img src="imgs/pcb_03.png" alt="Altium PCB example 3" width="420" /></td>
  </tr>
  <tr>
    <td><img src="imgs/pcb_04.png" alt="Altium PCB example 4" width="420" /></td>
    <td><img src="imgs/pcb_05.png" alt="Altium PCB example 5" width="420" /></td>
    <td><img src="imgs/pcb_06.png" alt="Altium PCB example 6" width="420" /></td>
  </tr>
</table>

<table>
  <summary>Altium SchLib examples</summary>
  <tr>
    <td><img src="imgs/sch_01.png" alt="Altium SchLib example 1" width="420" /></td>
    <td><img src="imgs/sch_02.png" alt="Altium SchLib example 2" width="420" /></td>
    <td><img src="imgs/sch_03.png" alt="Altium SchLib example 3" width="420" /></td>
  </tr>
  <tr>
    <td><img src="imgs/sch_04.png" alt="Altium SchLib example 4" width="420" /></td>
    <td><img src="imgs/sch_05.png" alt="Altium SchLib example 5" width="420" /></td>
    <td><img src="imgs/sch_06.png" alt="Altium SchLib example 6" width="420" /></td>
  </tr>
</table>

## References And Thanks

- Thanks to [`easyeda2kicad`](https://github.com/uPesy/easyeda2kicad.py) and [uPesy](https://github.com/uPesy) for the project's original inspiration.
- Thanks to everyone who helped test unusual components, footprints, symbol directions, 3D models, and cross-platform release packages.
