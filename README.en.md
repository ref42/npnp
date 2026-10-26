<p align="center">
  <img src="assets/npnp.png" alt="npnp icon" width="260" />
</p>

<h1 align="center">npnp (Normalize Pin Net Pad)</h1>

<div align="center">

[![release](https://img.shields.io/github/v/release/ref42/npnp?display_name=tag)](https://github.com/ref42/npnp/releases) [![crates.io](https://img.shields.io/crates/v/npnp)](https://crates.io/crates/npnp) [![license](https://img.shields.io/badge/license-Non--Commercial-blue)](LICENSE) [![CI](https://img.shields.io/github/actions/workflow/status/ref42/npnp/ci.yml)](https://github.com/ref42/npnp/actions/workflows/ci.yml) [![downloads](https://img.shields.io/github/downloads/ref42/npnp/total)](https://github.com/ref42/npnp/releases)

![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust&logoColor=white) ![CLI](https://img.shields.io/badge/UI-command%20line-4b5563) ![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS-0078D6?logo=windows&logoColor=white) ![tokio](https://img.shields.io/badge/Async-tokio-14b8a6) ![reqwest](https://img.shields.io/badge/HTTP-reqwest-2ea44f)

</div>

<p align="center"><a href="README.en.md">English</a> | <a href="README.md">简体中文</a></p>

---

**npnp** batch-exports LCSC components into formats that Altium Designer (AD), KiCad and Cadence (OrCAD Capture / Allegro) can use — built for scripts, batches and `CI`.

## ✨ Features

### KiCad

- [x] Export KiCad-compatible `Symbol`, `Footprint`, and `3D` model formats.
- [x] Export individual components or batch export from a text file.
- [x] Append or update existing `.kicad_sym`, `.pretty`, and `.3dshapes` libraries.
- [x] Write STEP models directly into `.3dshapes` and reference them from footprints.
- [x] Merge existing KiCad symbol, footprint, and 3D libraries from a directory.

### Altium Designer (AD)

- [x] Export individual `.SchLib` and `.PcbLib` libraries. `.PcbLib` embeds upstream STEP data when available.
- [x] Support batch export.
- [x] Export multiple components into one merged `.SchLib` / `.PcbLib` pair.
- [x] Append new components to an existing npnp merged library without replacing the old components.
- [x] Merge existing Altium `.SchLib` / `.PcbLib` files from a directory into one library pair.

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

## 🔄 Workflow

<div align="center">
  <img src="imgs/workflow.png" alt="npnp/seex workflow: LCSC part numbers → seex / npnp → KiCad, Altium, Cadence" width="900"/>
</div>

## 📸 Export Examples

<table>
  <summary>KiCad symbol and footprint examples</summary>

  <table>
    <tr>
      <td><img src="imgs/showcase/kicad/sch_pcb/sch_pcb_1.png" alt="KiCad symbol and footprint example 1" width="420" /></td>
      <td><img src="imgs/showcase/kicad/sch_pcb/sch_pcb_2.png" alt="KiCad symbol and footprint example 2" width="420" /></td>
      <td><img src="imgs/showcase/kicad/sch_pcb/sch_pcb_3.png" alt="KiCad symbol and footprint example 3" width="420" /></td>
    </tr>
    <tr>
      <td><img src="imgs/showcase/kicad/sch_pcb/sch_pcb_4.png" alt="KiCad symbol and footprint example 4" width="420" /></td>
      <td><img src="imgs/showcase/kicad/sch_pcb/sch_pcb_5.png" alt="KiCad symbol and footprint example 5" width="420" /></td>
      <td><img src="imgs/showcase/kicad/sch_pcb/sch_pcb_6.png" alt="KiCad symbol and footprint example 6" width="420" /></td>
    </tr>
  </table>
</table>

<table>
  <summary>KiCad 3D model examples</summary>

  <table>
    <tr>
      <td><img src="imgs/showcase/kicad/3d_models/SOT-89-3.png" alt="SOT-89-3" width="420" /></td>
      <td><img src="imgs/showcase/kicad/3d_models/TSSOP-20.png" alt="TSSOP-20" width="420" /></td>
      <td><img src="imgs/showcase/kicad/3d_models/QFN-80.png" alt="QFN-80" width="420" /></td>
    </tr>
    <tr>
      <td><img src="imgs/showcase/kicad/3d_models/SOIC-8.png" alt="SOIC-8" width="420" /></td>
      <td><img src="imgs/showcase/kicad/3d_models/SOIC-14.png" alt="SOIC-14" width="420" /></td>
      <td><img src="imgs/showcase/kicad/3d_models/LQFP-48.png" alt="LQFP-48" width="420" /></td>
    </tr>
  </table>
</table>

<table>
  <summary>Altium SCH + 3D model examples</summary>

  <table>
    <tr>
      <td><img src="imgs/showcase/ad/sch_3d/2N2222AU.png" alt="2N2222AU" width="420" /></td>
      <td><img src="imgs/showcase/ad/sch_3d/CH32V003F4P6.png" alt="CH32V003F4P6" width="420" /></td>
      <td><img src="imgs/showcase/ad/sch_3d/ESP32-S31NRV16.png" alt="ESP32-S31NRV16" width="420" /></td>
    </tr>
    <tr>
      <td><img src="imgs/showcase/ad/sch_3d/NE555DR.png" alt="NE555DR" width="420" /></td>
      <td><img src="imgs/showcase/ad/sch_3d/SN74HC04DR.png" alt="SN74HC04DR" width="420" /></td>
      <td><img src="imgs/showcase/ad/sch_3d/STM32F103C8T6.png" alt="STM32F103C8T6" width="420" /></td>
    </tr>
  </table>
</table>

<table>
  <summary>Altium PCB examples</summary>

  <table>
    <tr>
      <td><img src="imgs/showcase/ad/pcd/sot89-3.png" alt="sot89-3" width="420" /></td>
      <td><img src="imgs/showcase/ad/pcd/tssop-20.png" alt="tssop-20" width="420" /></td>
      <td><img src="imgs/showcase/ad/pcd/qfn-80.png" alt="qfn-80" width="420" /></td>
    </tr>
    <tr>
      <td><img src="imgs/showcase/ad/pcd/soic-8.png" alt="soic-8" width="420" /></td>
      <td><img src="imgs/showcase/ad/pcd/soic-14.png" alt="soic-14" width="420" /></td>
      <td><img src="imgs/showcase/ad/pcd/lqfp-48.png" alt="lqfp-48" width="420" /></td>
    </tr>
  </table>
</table>

## 🖥️ Supported EDA Versions

> [!NOTE]
> These are the versions currently known to work. npnp may support additional versions. If a library exported by npnp cannot be opened by an older EDA version, please open an issue so compatibility can be considered.

- [x] `KiCad 9.0` to `Latest`
- [x] `Altium Designer 23.x` to `Latest`
- [x] `Cadence SPB 26.1` (OrCAD X Capture / Allegro X); the `Allegro` footprint scripts need a release that supports `SKILL` (`Allegro PCB Designer` or `OrCAD PCB Designer Professional`)

## 🚀 Install and Use

### Downloads

Every tag release builds these native `npnp` CLI archives:

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

### Install

```bash
cargo install npnp
```

Or build from source:

```bash
git clone https://github.com/ref42/npnp.git
cd npnp
cargo build --release
```

The executable is written to `target/release/npnp.exe`.

### CLI

Type `npnp p` to print ready-to-run commands. Export commands are grouped by target EDA tool:

```bash
npnp search C2040 --limit 5

npnp altium C2040 -o altium_libs
npnp altium -i ids.txt -o altium_libs --name MyParts -j 42
npnp altium -i new_ids.txt -o altium_libs --name MyParts --append -j 42

npnp merge -i altium_parts -o MyParts
npnp merge -i kicad_parts -o MyParts

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

### Merge existing libraries

```bash
npnp merge -i path/to/libraries -o merged/MyParts
npnp merge -i path/to/libraries -o merged/MyParts --force
```

Put libraries directly in the input directory. The command detects Altium or KiCad; keep the two formats in separate directories. This command works offline and does not convert between EDA formats.

- Altium: `.SchLib` and `.PcbLib` files become `MyParts.SchLib` and `MyParts.PcbLib`.
- Altium excludes the requested output files on reruns. Repeated component names keep the first serialized record, which lets an earlier merged library coexist with its source libraries when choosing a new output name.
- KiCad 6 and later: `.kicad_sym` files, `.pretty` directories, and `.3dshapes` directories become `MyParts.kicad_sym`, `MyParts.pretty`, and `MyParts.3dshapes`. Include whichever types you need. Model subdirectories are preserved.
- `-o` is an output **base path**, not an output directory. Existing outputs require `--force`. KiCad excludes the requested output libraries from its inputs, so rerunning into the same directory works. Replacement removes stale files from the output directories; nesting an output inside a source library is rejected.
- Identical same-named KiCad entries are kept once, comparing symbols and footprints after reference updates and ignoring whitespace. This allows previous merged libraries to coexist with their sources when choosing a new output name. Different contents under the same name still stop the merge before outputs change, even with `--force`. Legacy `.lib` symbols and unsupported library-level metadata are not merged.
- Symbol footprint properties are updated when their target footprint is included. Included model references use absolute paths to the copied files, so no new KiCad path variable is needed. External references stay unchanged. If you move the merged files, update those model paths in KiCad or rerun the merge at the new destination.

Register `MyParts.kicad_sym` in KiCad's symbol library table and `MyParts.pretty` in its footprint library table using the nickname `MyParts`.

### Export contract

- Generated component and model names are deterministic: `<source-name>__<LCSC-ID>`. The ID is also retained in the schematic metadata as `NPNP_COMPONENT_ID`.
- Missing STEP data does not prevent symbol or footprint export; the console summary identifies missing-model counts where the target supports 3D output.
- The Cadence target writes `<library>.xml` (Capture Library XML), `<library>_allegro/` (one `SKILL` script per footprint plus the library script) and `<library>_step/` (the `3D` models). `npnp` calls no Cadence program: the `.olb` comes from `File > Import > Library XML` in Capture, and the `.dra`/`.psm`/`.pad` files from `skill load("<folder>/<library>.il")` in the Allegro command window.
- Fresh merged output refuses to overwrite an existing library. Use `--append` to preserve the existing library collection and add new components, or `--force` when replacing standalone generated files is intentional.
- Generated libraries target real EDA use, but a visual check is still recommended before production.

## 📖 Before You Start

> [!NOTE]
> The source is public again, but please follow the [LICENSE](LICENSE) when using it.

`npnp` stands for `Normalize Pin Net Pad`: the CLI I built while reverse-engineering the `Altium Designer` file format and tidying up the LCSC/EasyEDA export flow. It is for people who want scripts, batches, `CI` or simply a command line; if you want a GUI, use the same family's [`SeEx`](https://github.com/ref42/seex).

`npnp` and `SeEx` aim to export the same things, in particular:

- `3D` models are placed on the board plane before they are written, and the LCEDA logo is removed by default, so both front ends produce the same model.
- `KiCad` and `Altium` download `STEP` through the same LCSC model endpoints and HTTP setup, falling back to the `easyeda.com` mirror when `lceda.cn` is unreachable.
- `Altium`, `KiCad` and `Cadence` all export metadata in Chinese or English; when the target language is missing the source data is kept as a fallback.
- Generated libraries target real EDA use, but a visual check is still recommended before production.

If an exported symbol, footprint, 3D model or metadata field looks wrong, please open an `issue` with the LCSC ID, the target EDA, the command and a screenshot. PRs are not merged by default unless the direction has been agreed first.

## 🧩 Sister Project

- [`SeEx`](https://github.com/ref42/seex) is the `GUI` version of `npnp`, kept in step with it, for people who prefer a window.
  - Clipboard monitoring, part search, previews and export all happen in one place.
  - For the `Cadence` target both projects write the same `Library XML` and `SKILL` scripts.
  - `npnp` is the one to call from `AI`/agents: let the `AI` analyse a project, then have it run `npnp` to export the parts it needs.

## 🗺️ Roadmap

- [ ] Keep improving the Altium / KiCad / Cadence workflows and fix more hidden `bugs`.
- [ ] Improve solder mask handling for irregular pads during `.PcbLib` export.
- [ ] Support more `EDA` tools, such as `PADS`.
- [ ] Add more regression fixtures for unusual symbols, footprints, and `3D` models.
- [ ] Improve documentation for batch merge, append, and CI workflows.

## 🤝 Contributing

- [x] Has software design experience and can help maintain the software over time.
- [x] Has good taste; your suggestions would be very welcome.
- [x] If you have tokens to spare and are interested in this project, feel free to give it a go: the project does not reject `AI`, it only asks that every author on a submitted `PR` is a real living person.
- [x] Contributing code is not required — every suggestion that makes `npnp` and `SeEx` better will be considered, and will be thanked at the end of this file.

## 🙏 Acknowledgements

The entries are in no particular order; they follow the development timeline.

### References

- [`easyeda2kicad`](https://github.com/uPesy/easyeda2kicad.py)@[uPesy](https://github.com/uPesy)
  - Although it only supports `KiCad`, it gave me some of the inspiration. It is an excellent tool, and the vast majority of EasyEDA import plugins for `KiCad` on the market are built on top of it.
- `IPC-7352(2023)` https://dokumen.pub/qdownload/ipc-7352-generic-guideline-for-land-pattern-design.html
  - Used for the `Cadence` land calculations and for rebuilding the `3D` models.

### Thanks

- 菊猫
  - Gave me part of my `TOKEN`. I also paid for some of it myself, but in the end it worked out as roughly half bought and half donated. `opus 4.6` was still very good at the time, and I leaned on it heavily to implement a lot of features and lay a good foundation.
- emoji老弟@[z217-cpu](https://github.com/z217-cpu)
  - Helped me test multi-platform compatibility from the very start of the project until now; thank you very much!
- 🐏🐏@[Cloud-Lightning](https://github.com/Cloud-Lightning)
  - Has used `seex`/`npnp` in several projects and gave me a great deal of practical advice; thank you very much!
- c大佬@[lins-code54](https://github.com/lins-code54)
  - Provided the `Cadence` export support for `SeEx`, which npnp's `Cadence` target comes from, and the `3D` model rebuild feature; thank you very much!
- All users
  - Without your downloads and `star`s this project might have been shelved; thank you very much!

## 📄 License

This project uses the custom **npnp Non-Commercial Share-Alike License 1.0**: studying, modifying and redistributing are allowed, **commercial use of any kind is not**, and modified works must be shared under the same license. The full terms are in [LICENSE](LICENSE).

> [!NOTE]
> Although the source code is public, please follow the [LICENSE](LICENSE) when using this project.

## Copyright Notice

`Altium` and `Altium Designer` are trademarks of Altium Limited; `Cadence`, `OrCAD` and `Allegro` are trademarks of Cadence Design Systems, Inc.; `LCSC`, `LCEDA` and `JLC` are trademarks of Shenzhen JLC Technology Group Co., Ltd. `KiCad` is the name and mark of the KiCad project and its developer community, released under the GPL. This project is not affiliated with or endorsed by any of these companies or projects; the names are used only to describe compatibility.
