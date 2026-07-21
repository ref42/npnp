# npnp

<p align="center">
  <img src="assets/npnp.png" alt="npnp app logo" width="360">
</p>

<p align="center">
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


## CLI

Type `npnp --prompt` to print ready-to-run commands. Export commands are grouped by target EDA tool:

```bash
npnp search C2040 --limit 5

npnp altium export C2040 --full --output altium-libs --force
npnp altium batch --input ids.txt --output generated\altium --merge --library-name MyLib --full --continue-on-error
npnp altium batch --input new_ids.txt --output generated\altium --merge --append --library-name MyLib --full --force --continue-on-error

npnp kicad export C2040 --full --output kicad-libs --library-name MyParts --force
npnp kicad batch --input ids.txt --output generated\kicad --library-name MyParts --full --force --parallel 4 --continue-on-error
```

Useful shared flags:

- `--full`: export all supported library targets for that EDA tool.
- `--force`: replace existing generated outputs.
- `--english-metadata`: prefer English metadata when available, while falling back to source metadata when it is missing.
- `--library-name`: set the merged Altium library name or KiCad library base name.
- Altium `--merge` creates a new merged library and refuses to overwrite an existing one. Use `--merge --append` to add components to an existing merged library.

