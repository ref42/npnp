# Cadence 16.6（OrCAD Capture 16.6-2015）转换与验证方法

上游 npnp 只在 Cadence 17.x 及以上版本测试过。本目录是 `cadence-16.6`
分支在 **OrCAD Capture 16.6-2015（SPB 16.6-2015 S066）** 上转换与验证
的完整方法——该版本导出 .olb 的唯一入口是 Capture 内部技能命令
`XMATIC_XML2OLB`（无公开命令行）。

## 文件夹内容

| 文件 | 用途 |
|---|---|
| `normalize_xml.py` | 16.6 方言归一：剥掉 16.6 导入器拒绝的内容（`CellName` 属性、`timestamp`/`timezone`/`charset`/`ValueIfValueExist` 属性、`<Ellipse>` 元素）；门号属性原样透传。用法：`python normalize_xml.py <输入.xml> <输出.xml>` |
| `xml2olb.tcl` | 批量转 .olb：在 Capture 16.6 会话里 `source` 本脚本（当前目录放归一后的 XML），每个 XML 经 `XMATIC_XML2OLB` 转成同名 .olb，结果在 `xml2olb_result.txt` |
| `olb_verify.tcl` | 无界面验证：不开 Capture，加载 DBO 接口（`orDb_Dll_Tcl.dll`）枚举每个 .olb 的包与 part 名，自动标出 npnp#4 症状（`-1.Normal` 改名重复 part）。改文件头的 `DIR` 指向任意目录，用 Capture 自带 tclsh8.4 运行 |
| `README.zh-CN.md` / `README.md` | 本说明（中/英） |

## 转换链

1. `npnp cadence -i ids.txt -o <目录> --name <库名> --force` → Library XML
2. 16.6 方言归一（16.6 的导入器比 17.x 严格）：
   `python normalize_xml.py <目录>/<库名>.xml <目录>/<库名>.16.xml`
   - 剥掉 `<LibPart>` 上 16.6 拒绝的 `CellName` 属性
   - 剥掉 `timestamp`/`timezone`/`charset`/`ValueIfValueExist` 属性
   - 删除 `<Ellipse>` 元素（16.6 XML2OLB 遇到会失败）
   - 门号属性（`alphabeticNumbering` / `isHomogeneous`）**原样透传**：
     `cadence-16.6` 分支已原生输出 16.6 正确值——单门
     `alphabeticNumbering="2" isHomogeneous="1"`、异构多分栏 `1`/`0`
3. 在 Capture 16.6 会话中 `source xml2olb.tcl`——目录里每个 `*.xml`
   经 `XMATIC_XML2OLB` 转成同名 `.olb`（目标存在则追加、不存在则新建），
   结果写入 `xml2olb_result.txt`

## 验证链

- `olb_verify.tcl`（无界面，不需打开 Capture）：加载 `orDb_Dll_Tcl.dll`，
  枚举每个 .olb 的全部包与 part 名，输出 `olb_verify_result.txt`。
  npnp#4 症状守门：出现 `<包名>-1.Normal` 改名重复 part 即为异常；
  单芯 part 名应为 `<包名>.Normal`、异构分段应为 `<包名><字母>.Normal`
- Capture 放置抽验：位号不带门号后缀（npnp#3 症状：单芯件显示 `C1-1`/`C1A`）

## 本分支修复的 16.6 实测证据

| 分支修复 | 样本 | 结果 |
|---|---|---|
| 封装名缺暴露焊盘尺寸 → 从画稿测量并注入 `EP<w>x<h>` | C5153652（SGM61022，TDFN-8 2x2，title 带 `-EP`） | EP 量得 1.10 × 1.99 mm → SON 9 焊盘计算成功（对位差 0.36 > 0.35 容差 → 按设计回退厂家焊盘） |
| 整图子件引脚被并进每个分栏（并集 + `_2` 改名重复脚） | C12594（TL074CDR，4 分栏 + 整图子件） | 4 分栏 [5, 3, 3, 3] 脚、零改名脚 |
| 门号枚举按分栏类型输出 | 上表全部 + 350 只基础库回归 | 单芯 `2/1`、异构 `1/0`；与 Capture 原生库 OLB2XML 导出对照一致（175 个单门件均为 `2/1`） |
| 回归 | C7950（SOIC-8）、C42379236、C5152799、C7420354 | 与 v3.1.0 输出一致 |

环境：OrCAD Capture 16.6-2015 S066；`XMATIC_XML2OLB` / `XMATIC_OLB2XML`
为 16.6 内部技能入口；验证脚本使用 Capture 自带 tclsh8.4 + 加载
`orDb_Dll_Tcl.dll`（`load ... DboTclWriteBasic`）做 DBO 读回。
