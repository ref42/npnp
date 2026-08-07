<p align="center">
  <img src="assets/npnp.png" alt="npnp 图标" width="260" />
</p>

<h1 align="center">npnp (Normalize Pin Net Pad)</h1>

<p align="center">
  <strong>批量导出立创商城元件为 Altium Designer（AD）、KiCad 可用的命令行工具。</strong>
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

### 共同部分

- [x] 支持选择中文或英文元数据导出。
- [x] 支持多线程/并行下载和批处理。
- [x] 支持单独导出 `3D` 模型。
- [x] 支持导出原始 symbol / footprint JSON，便于问题排查。
- [x] 支持 Windows、macOS、Linux 多平台发布包。

## npnp 支持的 EDA 版本

> [!NOTE]
> 这些版本仅为目前所知，不排除 `npnp` 支持更多版本。如果你无法在更低版本的 EDA 软件中打开导出的库，可以在 `issue` 里提出。

- [x] `KiCad 9.0` ~ `Latest`
- [x] `Altium Designer 23.x` ~ `Latest`

## npnp 未来想做啥？

- [ ] 继续完善现有 Altium / KiCad 工作流，修复更多隐藏的 `bug`。
- [ ] 改进不规则焊盘导出 `.PcbLib` 时的阻焊处理。
- [ ] 尽可能移除或隐藏下载 3D 模型中的 logo/watermark 几何体。
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

运行 `npnp --prompt` 可以打印可直接复制的命令。导出命令按目标 EDA 工具分组：

```bash
npnp search C2040 --limit 5

npnp altium export C2040 --full --output altium-libs --force
npnp altium batch --input ids.txt --output generated/altium --merge --library-name MyLib --full --continue-on-error
npnp altium batch --input new_ids.txt --output generated/altium --merge --append --library-name MyLib --full --force --continue-on-error

npnp kicad export C2040 --full --output kicad-libs --library-name MyParts --force
npnp kicad batch --input ids.txt --output generated/kicad --library-name MyParts --full --force --parallel 4 --continue-on-error
```

常用统一参数：

- `--full`：导出该 EDA 工具支持的全部库目标。
- `--force`：替换已有生成结果。
- `--english-metadata`：优先使用英文元数据；如果缺失，则自动回退到源数据中的元数据。
- `--library-name`：设置 Altium 合并库名称或 KiCad 库基础名称。
- Altium `--merge` 只创建新的合并库，不会覆盖已有同名库；要给已有合并库追加元件，请使用 `--merge --append`。

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
