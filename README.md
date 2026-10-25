<p align="center">
  <img src="assets/npnp.png" alt="npnp 图标" width="260" />
</p>

<h1 align="center">npnp (Normalize Pin Net Pad)</h1>

<p align="center">
  <strong>批量将立创商城元件导出为 Altium Designer（AD）、KiCad、Cadence（OrCAD Capture / Allegro）可用格式的命令行工具。</strong>
</p>

<p align="center">
  <a href=".github/workflows/release.yml"><img src="https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-2ea44f?style=flat-square" alt="platform Windows macOS Linux"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/rust-edition%202024-f28d1a?style=flat-square" alt="rust edition 2024"></a>
</p>

<p align="center"><a href="README.en.md">English</a> | <a href="README.md">简体中文</a></p>

---

## 写在开头

> [!NOTE]
> 项目源码已重新公开，但请务必遵循 [LICENSE](LICENSE) 进行使用。

`npnp` 是 `Normalize Pin Net Pad` 的缩写，也是我在逆向 `Altium Designer` 文件格式、整理立创/EasyEDA 数据导出流程时做出的 CLI 工具。它适合需要脚本化、批处理、CI 或者更喜欢命令行的用户；如果你需要 GUI，可以使用同系列的 [`SeEx`](https://github.com/ref42/seex)。

目前 `npnp` 和 `SeEx` 的导出行为会尽量保持一致，尤其是以下部分：

- KiCad `3D` 导出会直接写入下载到的 STEP 数据，不再对 STEP 做耗时的二次解析和重写。
- KiCad STEP 下载使用与 Altium 相同的 LCSC 模型接口和 HTTP 连接方式。
- Altium、KiCad 都支持中英文元数据导出；当目标语言元数据缺失时，会保留并回退到已有源数据。
- 生成的库文件面向实际 EDA 使用，但在用于生产前仍然建议目检确认。

如果你发现导出的符号、封装、3D 模型或元数据有问题，欢迎提 `issue`，请尽量附上 LCSC 编号、目标 EDA、导出命令和截图。PR 默认不会直接合并，除非事先沟通确认维护方向。

## npnp 现在能做啥？

### KiCad 部分

- [x] 导出 KiCad 可用的 `Symbol`、`Footprint`、`3D` 模型。
- [x] 支持单个器件导出，也支持从文本文件批量导出。
- [x] 支持指定现有 `.kicad_sym`、`.pretty`、`.3dshapes` 库并追加/更新器件。
- [x] STEP 模型直接写入 `.3dshapes`，并在封装中生成模型引用。

### Altium Designer（AD）部分

- [x] 单独导出器件的 `.SchLib` 和 `.PcbLib`，`.PcbLib` 会嵌入上游 STEP 模型。
- [x] 支持批量导出。
- [x] 支持多个器件合并导出到同一组 `.SchLib` / `.PcbLib`。
- [x] 支持向已有的 npnp 合并库追加新器件，避免因为同名库覆盖而丢失已有器件。

### Cadence（OrCAD Capture / Allegro）部分

- [x] 导出 `Capture` 原理图符号：一个 `Library XML` 文件，在 `Capture` 中通过 `File > Import > Library XML` 转换为 `.olb`；立创 EDA 的多 part 符号会导出为同一个器件的多个 section。
- [x] 导出 `Allegro` 封装：每个封装一个 `SKILL` 脚本，外加一个库脚本；在 `Allegro PCB Editor` 命令窗口执行 `skill load("<库>_allegro/<库>.il")`，即在同一目录生成 `.dra`、`.psm` 和 `.pad`。
- [x] 导出 `3D` 模型（`step`），封装中已按立创 EDA 的位置映射好模型。
- [x] 标准封装（贴片阻容、`SOT`、`SOD`、`SOIC`、`QFN` 等）按 `IPC-7352` 计算焊盘并核对厂家焊盘；其余封装保留厂家焊盘，按 `IPC-7352` 规则生成丝印、装配层和占位区。
- [x] `npnp` 本身不调用任何 `Cadence` 程序，导入由用户在 `Cadence` 中完成。

### 共同部分

- [x] 支持选择中文或英文元数据导出。
- [x] 支持多线程/并行下载和批处理。
- [x] 支持单独导出 `3D` 模型。
- [x] 默认去除下载 `3D` 模型上的立创 EDA 标志（水印）并重建模型；无法安全去除时保留原模型，并在导出结果中列出这些元件。`--keep-model-logo` 可保留原始模型。
- [x] 支持导出原始 symbol / footprint JSON，便于问题排查。
- [x] 支持 Windows、macOS、Linux 多平台发布包。

## npnp 支持的 EDA 版本

> [!NOTE]
> 这些版本仅为目前所知，不排除 `npnp` 支持更多版本。如果你无法在更低版本的 EDA 软件中打开导出的库，可以在 `issue` 里提出。

- [x] `KiCad 9.0` ~ `Latest`
- [x] `Altium Designer 23.x` ~ `Latest`
- [x] `Cadence SPB 26.1`（OrCAD X Capture / Allegro X）；`Allegro` 封装脚本需要支持 `SKILL` 的版本（`Allegro PCB Designer` 或 `OrCAD PCB Designer Professional`）

## npnp 未来想做啥？

- [ ] 继续完善现有 Altium / KiCad 工作流，修复更多隐藏的 `bug`。
- [ ] 改进不规则焊盘导出 `.PcbLib` 时的阻焊处理。
- [ ] 支持更多 `EDA` 工具，例如 `PADS`。
- [ ] 增加更多异常符号、封装、3D 模型的回归测试样例。
- [ ] 完善批量合并、追加和 CI 使用文档。

## 下载

每个 tag release 会构建这些原生 `npnp` CLI 压缩包：

- Windows x86_64：`npnp-vX.Y.Z-windows-x86_64.zip`
- Windows ARM64：`npnp-vX.Y.Z-windows-aarch64.zip`
- Linux x86_64：`npnp-vX.Y.Z-linux-x86_64.tar.gz`
- Linux ARM64：`npnp-vX.Y.Z-linux-aarch64.tar.gz`
- Fedora x86_64：`npnp-vX.Y.Z-fedora-x86_64.tar.gz`
- Fedora ARM64：`npnp-vX.Y.Z-fedora-aarch64.tar.gz`
- Arch Linux x86_64：`npnp-vX.Y.Z-arch-x86_64.tar.gz`
- Arch Linux ARM64：`npnp-vX.Y.Z-arch-aarch64.tar.gz`
- macOS Intel：`npnp-vX.Y.Z-macos-x86_64.tar.gz`
- macOS Apple Silicon：`npnp-vX.Y.Z-macos-aarch64.tar.gz`

## 命令示例

运行 `npnp p` 可以打印可直接复制的命令。导出命令按目标 EDA 工具分组：

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

常用统一参数：

- `--full`：导出该 EDA 工具支持的全部库目标。
- `--force`：替换已有生成结果。
- `--language auto|en|zh`：选择元数据语言。`auto` 使用源元数据，`en` 获取英文元数据并在缺失时回退到源元数据。
- `-j`：使用 `-i` 时的最大并发导出数。
- `-i`：从任意纯文本文件读取器件 ID，文件名不要求使用 `.txt` 扩展名。
- `--name`：可选参数，用于覆盖生成库的基础名称。
- `--append`：更新 `-o` 指定目录中的库集合。Altium 会发现合并的 `.SchLib`/`.PcbLib`，KiCad 会发现 `.kicad_sym`、`.pretty` 和 `.3dshapes` 库，Cadence 会向已有的 `<库名>.xml` 和 `_allegro` 目录追加。
- `--keep-model-logo`：保留下载 `3D` 模型上的立创 EDA 标志（默认会去除）。

### 导出约定

- 生成的器件和模型名称是确定性的：`<source-name>__<LCSC-ID>`。原理图库元数据中仍会保留 `NPNP_COMPONENT_ID`。
- 缺少 STEP 数据不会阻止符号或封装导出；目标支持 3D 导出时，终端摘要会标明缺少模型的数量。
- Cadence 目标写出 `<库名>.xml`（Capture Library XML）、`<库名>_allegro/`（每个封装一个 `SKILL` 脚本加一个库脚本）和 `<库名>_step/`（`3D` 模型）。`npnp` 不调用 Cadence：`.olb` 由 Capture 的 `File > Import > Library XML` 生成，`.dra`/`.psm`/`.pad` 由 Allegro 命令窗口执行 `skill load("<目录>/<库名>.il")` 生成。
- 新建合并库不会覆盖已有库。使用 `--append` 保留已有库并添加新器件；只有在确实需要替换独立生成文件时才使用 `--force`。

## 导出示例

<table>
  <summary>KiCad 示例</summary>
  <tr>
    <td align="center"><img src="imgs/kicad_symbol.png" alt="KiCad 符号" width="280" /><br /><sub>符号库</sub></td>
    <td align="center"><img src="imgs/kicad_footprint.png" alt="KiCad 封装" width="280" /><br /><sub>封装库</sub></td>
    <td align="center"><img src="imgs/kicad_3dmodel.png" alt="KiCad 3D 模型" width="280" /><br /><sub>3D 模型</sub></td>
  </tr>
</table>

<table>
  <summary>Altium PCB 库示例</summary>
  <tr>
    <td><img src="imgs/pcb_01.png" alt="Altium PCB 示例 1" width="420" /></td>
    <td><img src="imgs/pcb_02.png" alt="Altium PCB 示例 2" width="420" /></td>
    <td><img src="imgs/pcb_03.png" alt="Altium PCB 示例 3" width="420" /></td>
  </tr>
  <tr>
    <td><img src="imgs/pcb_04.png" alt="Altium PCB 示例 4" width="420" /></td>
    <td><img src="imgs/pcb_05.png" alt="Altium PCB 示例 5" width="420" /></td>
    <td><img src="imgs/pcb_06.png" alt="Altium PCB 示例 6" width="420" /></td>
  </tr>
</table>

<table>
  <summary>Altium SchLib 示例</summary>
  <tr>
    <td><img src="imgs/sch_01.png" alt="Altium SchLib 示例 1" width="420" /></td>
    <td><img src="imgs/sch_02.png" alt="Altium SchLib 示例 2" width="420" /></td>
    <td><img src="imgs/sch_03.png" alt="Altium SchLib 示例 3" width="420" /></td>
  </tr>
  <tr>
    <td><img src="imgs/sch_04.png" alt="Altium SchLib 示例 4" width="420" /></td>
    <td><img src="imgs/sch_05.png" alt="Altium SchLib 示例 5" width="420" /></td>
    <td><img src="imgs/sch_06.png" alt="Altium SchLib 示例 6" width="420" /></td>
  </tr>
</table>

## 参考和致谢

- 感谢 [`easyeda2kicad`](https://github.com/uPesy/easyeda2kicad.py) 和 [uPesy](https://github.com/uPesy) 给了项目最开始的灵感。
- 感谢所有帮助测试异常器件、封装、符号方向、3D 模型和跨平台发布包的用户。
