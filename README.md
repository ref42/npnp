# npnp

<p align="center">
  <img src="assets/npnp.png" alt="npnp app logo" width="360">
</p>

<p align="center">
  <a href="Cargo.toml"><img src="https://img.shields.io/badge/version-1.0.2-e05d44?style=flat-square" alt="version 1.0.2"></a>
  <a href="LICENSE-APACHE"><img src="https://img.shields.io/badge/license-Apache--2.0-f08c5a?style=flat-square" alt="license Apache 2.0"></a>
  <a href=".github/workflows/release.yml"><img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-2ea44f?style=flat-square" alt="platform Windows macOS Linux"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-edition%202024-f28d1a?style=flat-square" alt="rust edition 2024"></a>
</p>

<p align="center">
  English | <a href="README.zh-CN.md">简体中文</a>
</p>

Normalize Pin Net Pad (`npnp`) is a pure Rust component library exporter for Altium and KiCad.

`npnp` searches component data, downloads upstream source files and 3D models, and exports ready-to-check schematic and PCB footprint libraries.

## Project status

### Implemented

- [x] Search components by keyword, part name, or LCSC ID.
- [x] Download 3D models as STEP or OBJ/MTL.
- [x] Export raw symbol and footprint JSON for inspection.
- [x] Export Altium schematic libraries (`.SchLib`).
- [x] Export Altium PCB footprint libraries (`.PcbLib`).
- [x] Export KiCad symbol, footprint, and 3D model libraries (`.kicad_sym`, `.pretty`, `.3dshapes`).
- [x] Embed STEP models into PCB libraries when upstream STEP data is available.
- [x] Batch export many component IDs from a text file.
- [x] Export either one file per component or merged library pairs.
- [x] Append new components into an existing merged library pair without duplicating existing component IDs.
- [x] Export optional English metadata with `--english-metadata`.

### Roadmap

- [ ] Remove logo/watermark geometry from downloaded 3D models when possible.
- [ ] Improve solder mask handling for irregular pads during `.PcbLib` export.
- [ ] Add more regression fixtures for unusual symbols and footprints.
- [ ] Improve documentation for batch merge and append workflows.

### Known limits

- Generated libraries should still be visually checked before fabrication.
- Some upstream symbols and footprints may use primitives that need special handling.

Schematic library screenshots:

<p align="center">
  <img src="imgs/sch_01.png" alt="Merged schematic screenshot 1" width="32%">
  <img src="imgs/sch_02.png" alt="Merged schematic screenshot 2" width="32%">
  <img src="imgs/sch_03.png" alt="Merged schematic screenshot 3" width="32%">
  <img src="imgs/sch_04.png" alt="Merged schematic screenshot 4" width="32%">
  <img src="imgs/sch_05.png" alt="Merged schematic screenshot 5" width="32%">
  <img src="imgs/sch_06.png" alt="Merged schematic screenshot 6" width="32%">
</p>

PCB library screenshots:

<p align="center">
  <img src="imgs/pcb_01.png" alt="Merged PCB screenshot 1" width="32%">
  <img src="imgs/pcb_02.png" alt="Merged PCB screenshot 2" width="32%">
  <img src="imgs/pcb_03.png" alt="Merged PCB screenshot 3" width="32%">
  <img src="imgs/pcb_04.png" alt="Merged PCB screenshot 4" width="32%">
  <img src="imgs/pcb_05.png" alt="Merged PCB screenshot 5" width="32%">
  <img src="imgs/pcb_06.png" alt="Merged PCB screenshot 6" width="32%">
</p>

KiCad library previews:

<p align="center">
  <img src="imgs/kicad_symbol.png" alt="KiCad symbol preview" width="32%">
  <img src="imgs/kicad_footprint.png" alt="KiCad footprint preview" width="32%">
  <img src="imgs/kicad_3dmodel.png" alt="KiCad 3D model preview" width="32%">
</p>

## Pull requests

Pull requests are welcome.

- Chinese and English are both welcome in issues and pull requests.
- Please keep changes focused and explain what problem the PR solves.
- For exporter changes, include the component ID or fixture used for testing when possible.
- If the change affects generated `.SchLib` or `.PcbLib` output, please visually check the result before submitting.
- For larger changes, opening an issue first is recommended so the scope can be discussed.

## Downloads

Tagged releases build native `npnp` CLI archives for:

- Windows x86_64: `npnp-vX.Y.Z-windows-x86_64.zip`
- Linux x86_64: `npnp-vX.Y.Z-linux-x86_64.tar.gz`
- Linux ARM64: `npnp-vX.Y.Z-linux-aarch64.tar.gz`
- macOS Intel: `npnp-vX.Y.Z-macos-x86_64.tar.gz`
- macOS Apple Silicon: `npnp-vX.Y.Z-macos-aarch64.tar.gz`

## CLI

Type `npnp --prompt` to print ready-to-run commands. Export commands are grouped by target EDA tool:

```bash
npnp search C2040 --limit 5

npnp altium export C2040 --full --output altium-libs --force
npnp altium batch --input ids.txt --output generated\altium --merge --library-name MyLib --full --force --continue-on-error
npnp altium batch --input new_ids.txt --output generated\altium --merge --append --library-name MyLib --full --force --continue-on-error

npnp kicad export C2040 --full --output kicad-libs --library-name MyParts --force
npnp kicad batch --input ids.txt --output generated\kicad --library-name MyParts --full --force --parallel 4 --continue-on-error
```

Useful shared flags:

- `--full`: export all supported library targets for that EDA tool.
- `--force`: replace existing generated outputs.
- `--english-metadata`: prefer English metadata when available, while falling back to source metadata when it is missing.
- `--library-name`: set the merged Altium library name or KiCad library base name.

## GUI

Another easy way is to use [`SeEx`](https://github.com/linkyourbin/seex). It is strongly recommended for day-to-day batch export.

I also made a showcase video on bilibili: [【工具分享】你应该把时间花费在电路设计和 Layout 上，而不是机械重复的绘制原理图和封装](https://www.bilibili.com/video/BV1bEEE6mEHd)
