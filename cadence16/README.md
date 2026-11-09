# Cadence 16.6 (OrCAD Capture 16.6-2015) — conversion & verification addendum

[简体中文版说明见 README.zh-CN.md / Simplified Chinese version: README.zh-CN.md]

Upstream npnp is only tested against Cadence 17.x+. This folder documents how
the `cadence-16.6` branch was converted and verified on **OrCAD Capture
16.6-2015** (SPB 16.6-2015 S066), where the only XML→OLB entry point is
Capture's internal `XMATIC_XML2OLB` skill command (no public CLI).

## Files in this folder

| File | Purpose |
|---|---|
| `normalize_xml.py` | Dialect normalization for the 16.6 importer: strips what 16.6 rejects (`CellName` attribute, `timestamp`/`timezone`/`charset`/`ValueIfValueExist` attributes, `<Ellipse>` elements); gate-numbering attributes pass through. Run: `python normalize_xml.py <in.xml> <out.xml>` |
| `xml2olb.tcl` | Bulk XML->OLB conversion. `source` it in a Capture 16.6 session with the CWD at the normalized `*.xml` files; each is converted to `<stem>.olb` via `XMATIC_XML2OLB`, log written to `xml2olb_result.txt` |
| `olb_verify.tcl` | Headless verification without opening Capture: loads `orDb_Dll_Tcl.dll` (DBO binding), enumerates every package/part of each `.olb` and flags renamed-duplicate part names (`*-1.Normal`, the npnp#4 symptom). Edit `DIR` at the top, run with Capture's bundled tclsh8.4 |
| `README.zh-CN.md` | Chinese version of this documentation |

## Conversion chain

1. `npnp cadence -i ids.txt -o <dir> --name <lib> --force` → Library XML
2. Normalize for the 16.6 dialect (stricter than 17.x):
   `python normalize_xml.py <dir>/<lib>.xml <dir>/<lib>.16.xml`
   - strips the `CellName` attribute on `<LibPart>` (16.6 rejects it)
   - strips `timestamp`/`timezone`/`charset`/`ValueIfValueExist` attributes
   - removes `<Ellipse>` elements (16.6 XML2OLB chokes on them)
   - gate-numbering attributes pass through unchanged: the `cadence-16.6`
     branch already emits what 16.6 expects (single-gate
     `alphabeticNumbering="2" isHomogeneous="1"`, heterogeneous `1`/`0`)
3. In a Capture 16.6 session, `source xml2olb.tcl` — converts every
   `*.xml` in the directory to `<stem>.olb` via `XMATIC_XML2OLB`
   (append mode when the target exists, create otherwise) and writes
   `xml2olb_result.txt`

## Verification chain

- `olb_verify.tcl` — headless (no Capture GUI): loads
  `orDb_Dll_Tcl.dll` standalone, enumerates every package and part of each
  `.olb`, writes `olb_verify_result.txt`. Gate for npnp#4: no part named
  `<pkg>-1.Normal` (the renamed-duplicate symptom), single-gate parts named
  `<pkg>.Normal`, heterogeneous sections named `<pkg><letter>.Normal`
- Capture placement spot-check: reference designators without a gate
  suffix (npnp#3 symptom: `C1-1` / `C1A` on single-gate parts)

## Verification evidence for this branch (16.6-2015 S066)

| Branch fix | Sample | Result |
|---|---|---|
| Exposed-pad size missing from name → measure from the drawing, inject `EP<w>x<h>` | C5153652 (SGM61022, TDFN-8 2x2, title `...-EP`) | EP measured 1.10 x 1.99 mm → SON, 9 lands computed (alignment check then rejected 0.36 mm > 0.35 tolerance → manufacturer lands kept by design) |
| Full-drawing subpart merged into every section (union + renamed `_2` pins) | C12594 (TL074CDR, 4 sections + full drawing) | 4 sections, 5/3/3/3 pins, zero renamed pins |
| Duplicate pin names within a section renamed with a numeric suffix ordered by pin number (GND, GND2, ...; NOT drawing order) | C61063 (XL1509-5, four GND pins) | pins GND/GND2/GND3/GND4 numbered by pin 5-8; C9002+C13738 (same family, different drawing order) rename identically and share one signature |
| Gate numbering enum per section type | all of the above + 350-part regression library | single-gate `2/1`, heterogeneous `1/0`; matches Capture-native OLBs (175 single-gate parts exported via OLB2XML for reference) |
| Regression | C7950 (SOIC-8), C42379236, C5152799, C7420354 | identical to v3.1.0 output |

Environment: OrCAD Capture 16.6-2015 S066, `XMATIC_XML2OLB` /
`XMATIC_OLB2XML` internal skill entries, Capture's bundled tclsh8.4 with
`load orDb_Dll_Tcl.dll DboTclWriteBasic` for the DBO readback.
