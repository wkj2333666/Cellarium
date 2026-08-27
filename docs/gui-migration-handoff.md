# Cellarium GUI 迁移交接说明

> 日期：2026-08-27
> 当前产品版本：v0.2.2
> 当前工作分支：`basis-workbench-implementation`
> GUI 迁移状态：设计与实施计划阶段，尚未开始迁移代码

## 1. 已经确定、不得重新猜测的产品决策

1. 主界面迁移到原生 GUI，采用 **egui/eframe + wgpu**。
2. TUI 废除。最终产品不再依赖 ratatui、crossterm、Kitty graphics、
   Sixel、iTerm2 graphics 或 half-block。
3. 产品不再提供服务器连接方式：
   - 删除 `cellarium server`；
   - 删除 `cellarium connect <host>`；
   - 删除 SSH connector 和远端二进制协议；
   - 不再把模拟放在 tinker、显示放在本机。
4. 模拟和显示全部在运行 GUI 的本机完成。
5. 自动计算后端顺序为：
   1. 可用的独立 GPU 原生后端，当前指 NVIDIA CUDA；
   2. 可用的 portable wgpu compute GPU，必须验证 Intel 集显，也允许
      Apple/AMD/树莓派集成 GPU 使用同一路径；
   3. CPU。
6. 用户可以在 Settings 中选择 Auto、CUDA、wgpu GPU 或 CPU。显式选择
   不可用后端时显示原因，不静默伪装成该后端。
7. 现有实验语义继续成立：
   - 默认 1 个通道；
   - 每个 `(basis polygon, output channel)` Binding 默认 1 个核；
   - 多核必须显式添加；
   - Potential 保留原始卷积和，不自动归一化；
   - Growth 支持 Rate 和 Value；
   - 严格 edge-to-edge 密铺，不支持 T-junction；
   - 默认 Tiling 设计为空白，可选预设或从头绘制。
8. GUI 必须鼠标优先。键盘是加速器，不得成为主要功能的唯一入口。
9. Agentic 测试必须用真实 GUI、真实键鼠和截图完成完整任务，不能以单元
   测试、事件回执、trace 或图片 hash 代替用户视觉判断。

## 2. 仓库和分支现状

远端工作树：

```text
/home/wkj/projects/cellarium/.worktrees/basis-workbench-implementation
```

基线与当前提交：

```text
271b79e  v0.2.2 / origin/main
8a41fb4  旧 TUI 图形化 Workbench 设计
9599ec1  旧 TUI 图形化 Workbench 计划
5a20955  TUI object strip
f959c00  Workbench 稳定选择与 decision state
4c58cf2  TUI Channels 图形卡片及通道生命周期修复
```

在作出 GUI 决定前，旧计划只执行到 Phase A / Task 3。Task 4 的 RED 测试
没有保留；工作树在写本交接文档前已恢复到已提交状态。

对迁移的价值判断：

| 提交 | 处理 |
| --- | --- |
| `8a41fb4`、`9599ec1` | 作为用户问题与交互需求来源；技术路线已被本 GUI 设计取代 |
| `5a20955` | TUI 布局/渲染删除；稳定对象 ID 和命中语义可参考 |
| `f959c00` | 保留模型层选择、撤销/重做、decision transaction 思路 |
| `4c58cf2` | 保留通道 ID、名称、冻结/解冻 Binding 修复；删除 ratatui 渲染 |

最近一次已完成的远端门禁是：

```sh
cargo fmt
cargo test --locked --lib
cargo test --locked --test workbench_e2e
git diff --check
```

这些测试验证的是旧 TUI 分支，不代表 GUI 已通过。

## 3. 构建与测试环境约束

- 本机是性能有限的 ARM64 树莓派，不在本机执行 Rust 构建。
- Rust 编译、单元测试、Clippy 和交叉构建放在 tinker 或 GitHub Actions。
- 树莓派只下载校验过 SHA256 的预编译 ARM64 Release 包。
- 最终 agentic 测试必须在树莓派真实 GUI 会话中运行该预编译包。
- tinker 可以继续作为开发构建机，但不能作为产品运行时模拟服务器。
- 不要用本机软件 Xvfb 的帧率推断真实 GPU 性能。

## 4. Cellarium 当前是什么

Cellarium 是一个可编辑的 cellular automata / continuous cellular
automata 实验室。它不只是 Conway 网格，还允许用户定义：

- 周期晶胞和密铺；
- 一个晶胞内多个具有独立状态语义的 basis polygon；
- 每个 basis 上的多个标量 Channel；
- 每个 `(basis, output channel)` 独立 RuleSet；
- RuleSet 中一个或多个 Kernel；
- Kernel 的 source channel、support、权重、采样度量和周期 offset；
- Rust 风格的 Growth 表达式；
- Rate 或 Value 更新模式；
- 初始世界、颜色、可见性、冻结状态和实验 dt。

### 4.1 核心数量关系

设：

- `B`：中心晶胞中的 basis polygon 数；
- `C_active`：非冻结通道数；
- `K(b,c)`：Binding `(basis=b, output=c)` 的核数。

则：

- Growth Binding 数量是 `B × C_active`；
- 每个 Binding 有独立 Growth；
- 每个 Binding 默认一个核；
- Growth 普通输入的数量恰好等于该 Binding 的核数；
- 完整签名是 `self + K(b,c)`；
- Channel 数和 Kernel 数没有相等关系；
- Kernel 可以从不同 source channel 读取；
- 多个 Binding 可共享 RuleSet，编辑时 copy-on-write 分离。

### 4.2 Growth 语义

```text
fn growth(self: Scalar, k1: Scalar, ..., kN: Scalar) -> Rate | Value
```

- `self` 是当前目标 basis/channel 的值；
- 每个 `kN` 是对应核的原始卷积结果；
- Rate：`next = clamp(self + dt * result, 0, 1)`；
- Value：`next = clamp(result, 0, 1)`；
- 程序最后一个无分号表达式是结果；
- 支持 `let`、`if/else`、算术、比较、逻辑和内置数学函数；
- 当前语言没有 `return`、循环、可变变量或副作用。

### 4.3 保存格式

默认数据目录：

- 绝对 `XDG_DATA_HOME`：`$XDG_DATA_HOME/cellarium/`；
- 否则：`$HOME/.local/share/cellarium/`。

文件：

- `workbench.ron`：active、draft 和 revision；
- `experiment.ron`：可独立打开运行的实验。

RON 数据模型应保持兼容。GUI 临时状态不写进实验文件；需要持久化的窗口和
编辑器偏好放独立 `settings.ron`。

## 5. 当前代码地图

### 5.1 必须保留

| 路径 | 责任 |
| --- | --- |
| `src/sim/experiment_model.rs` | ExperimentSpec、Channel、Kernel、Growth、更新模式 |
| `src/sim/ruleset.rs` | RuleSet、Binding、共享/default/local override |
| `src/sim/tiling/**` | 多边形、周期密铺、验证、coverage、solver、约束 |
| `src/sim/growth/**` | lexer、parser、AST、typecheck、eval、plot 采样 |
| `src/sim/basis_runtime.rs` | 多 basis 编译和 CPU 运行语义 |
| `src/sim/runtime.rs` | Experiment 编译与 CPU 参考实现 |
| `src/sim/cuda.rs`、`cuda_codegen.rs` | NVIDIA CUDA/NVRTC 后端 |
| `src/sim/service.rs` | Apply 原子性和 active/draft 切换的可复用逻辑 |
| `src/workbench/history.rs`、`command.rs` | 草稿事务与撤销/重做 |
| `src/workbench/state.rs` | 现有编辑动作与选择语义，迁移为 GUI Document 控制器 |
| `src/render/camera.rs`、`channels.rs`、`scene_transform.rs` | 可复用数学、颜色和坐标变换 |

### 5.2 重构后保留语义，不保留 UI 实现

| 当前路径 | GUI 目标 |
| --- | --- |
| `src/workbench/tiling_editor.rs` | 提取纯 scene/hit/command，egui Canvas 负责绘制 |
| `src/workbench/kernel_editor.rs` | 保留映射和编辑命令，删除 RGBA/TUI 假窗口依赖 |
| `src/workbench/growth_editor.rs` | 保留 source buffer、诊断和 plot model，使用 egui TextEdit |
| `src/workbench/channel_editor.rs` | 保留 ChannelCard view model 和生命周期命令 |
| `src/render/basis_scene.rs` | 拆为纯几何 scene + egui/wgpu renderer |
| `src/app.rs` | 拆为 GUI shell、Document、SimulationWorker，不保留巨型终端事件循环 |

### 5.3 最终删除

- `src/tui/**`
- `src/render/display/**`
- `src/remote.rs`
- 终端专用 `src/input.rs`
- `ratatui`、`ratatui-image`、`crossterm`
- Kitty/Sixel/iTerm2/half-block/shared-memory graphics
- SSH connector、远端协议、server loop
- PTY 和 Kitty protocol 测试
- `scripts/e2e-tinker.sh` 中的产品 C/S 旅程
- README、发布文档中的 `server` / `connect` 说明

删除必须发生在 GUI 功能和本地后端均已等价后，不能先删再长时间破坏主分支。

## 6. 目标程序结构

```text
cellarium
├── gui                 egui 应用、布局、控件、Canvas
├── document            active/draft、选择、历史、保存
├── simulation          本地 worker、命令、快照、性能指标
├── sim
│   ├── model           现有 Experiment/RuleSet/Tiling/Growth
│   ├── compile         后端无关 ComputePlan
│   └── backends
│       ├── cuda        NVIDIA
│       ├── wgpu        Intel/Apple/AMD/树莓派 GPU
│       └── cpu         参考与最终 fallback
└── persistence         RON workspace/experiment/settings
```

GUI 线程永远不执行模拟 step、CUDA 编译、WGSL pipeline 构建或大规模
readback。SimulationWorker 独占后端，通过命令队列接收 Apply、Run、Pause、
Step、Reset、WorldEdit，并通过 latest-only snapshot 暴露最新可显示状态。

## 7. 目标 GUI 信息架构

主窗口：

- 顶部：New、Open、Save、Undo、Redo、Apply & Run、Pause/Run、Step、
  Reset、Backend；
- 左侧：Simulation、Tiling、Channels、Kernels、Growth、Experiment；
- 中间：当前 section 的主要画布或编辑器；
- 右侧：简洁的对象属性与错误，不堆快捷键墙；
- 底部：backend、tick、sim Hz、frame Hz、draft 状态和持久错误。

所有 primary action 必须有可见鼠标入口和 tooltip。快捷键继续提供，但 Help
中显示，不占据 Inspector 主页面。

### 7.1 Tiling

- 默认空白；
- 明确的 Square、Triangle、Hexagon、Octagon+Square 预设卡片；
- 自由鼠标画多边形；
- 点击首点、双击或 Finish 按钮闭合；
- 非法顶点在放置时拒绝；
- 中心晶胞强调，周围真实邻接副本虚化；
- Solve seams 可见按钮；
- 解算后受约束顶点联动编辑；
- 严格 edge-to-edge，不支持 T-junction。

### 7.2 Channels

- 顶部显示所有通道卡片和 Add；
- 卡片直接选择、删除、改颜色、显隐、冻结；
- Composite、Solo、Grid 可点击；
- Live 和 Draft initial 明确分离，不静默替换；
- 显示真实 polygon geometry，不把六边形斜切成矩形纹理；
- Inspector 只显示 Channels 范围的数量。

### 7.3 Kernels

- 显示当前 Binding 的全部 Kernel 卡片和缩略图；
- Add 后新卡片立即可见并选中；
- 可以任意顺序点击切换；
- 删除当前选中核，不得误删“最后一个”；
- 引用中的核删除弹出可理解的决策对话框；
- Weights/Support、source/output、Affine/World、sigma、stencil/anchor 都是
  可见控件；
- cell 支持点击、拖动、滚轮细调、Shift/Ctrl 步长和双击精确输入；
- active、negative、zero、inactive、anchor、selected 有稳定图例。

### 7.4 Growth

- 中间上方完整显示函数签名；
- basis、output channel、self 和每个 kernel 是可点击 chip；
- source editor 有光标、选择、行号、语法色和 inline diagnostics；
- Plot 与源码同时处于中心区域；
- 0/1 个引用 kernel 默认曲线，2 个以上引用 kernel 默认 heatmap；
- 图的维度由实际引用和用户轴选择决定，不由总核数强行决定；
- 非轴输入有 pinned 数值编辑；
- stale、无有限样本、离散相等点都有明确视觉反馈；
- 语法手册在右侧 Help tab 滚动显示。

### 7.5 Experiment

- 汇总 basis、active/frozen channels、Bindings、当前/全部 kernels、
  Growth、dt、backend 和诊断；
- Apply & Run 是大而明确的按钮；
- Apply 是本地原子事务：构建新 backend 成功后才替换 active；
- Apply 失败保留旧 active simulation。

## 8. 后端选择和失败语义

Auto 探测顺序：

1. CUDA feature 已编译，NVIDIA driver、device 和 NVRTC 均可用；
2. wgpu 找到支持所需 compute/storage limits 的非 CPU adapter：
   - discrete adapter 优先；
   - integrated adapter 次之；
   - Intel 集显是强制发布验证目标；
   - Apple/AMD/树莓派集显允许使用相同 portable 路径；
3. CPU。

不把 UI renderer 等同于 compute backend。GUI 即使使用 wgpu 显示，也可能
因计算 limits 不足而选择 CPU。

每次探测产生结构化报告：

```rust
pub struct BackendProbe {
    pub kind: BackendKind,
    pub available: bool,
    pub device_name: Option<String>,
    pub reason: Option<String>,
}
```

Auto fallback 必须在 UI 中显示一次持久通知，例如
`CUDA unavailable (NVRTC missing); using Intel Iris Xe via wgpu`。

运行期 backend 错误：

- 暂停 worker；
- 保存最后一个已确认 snapshot；
- 尝试下一级 backend 从该 snapshot 重建；
- 成功后继续用户先前的运行/暂停状态；
- 失败则保持暂停并显示完整错误；
- 不跳过 tick，不发布半写状态。

## 9. 交接执行顺序

下一位 agent 应先阅读：

1. 本文件；
2. `docs/superpowers/specs/2026-08-27-local-egui-gui-migration-design.md`；
3. `docs/superpowers/plans/2026-08-27-local-egui-gui-migration.md`；
4. `docs/feature-inventory.md`（只用于产品语义；终端/C/S 部分已过时）。

随后：

1. 从当前 worktree 创建/继续专用 GUI 分支，不在 `origin/main` 直接开发；
2. 确认 `git status --short` 为空；
3. 不在树莓派运行 Cargo；
4. 严格按计划 TDD；
5. 每个阶段保持一个可启动、可回退 CPU 的本地 GUI；
6. GUI 等价后再删除 TUI/remote；
7. 最后对候选 Release 做完整 agentic GUI 旅程。

## 10. 旧文档的地位

以下内容作为历史设计保留，但不能指导新实现：

- `docs/remote-viewer.md`
- C1 remote viewer 设计和计划
- hybrid remote E2E 设计和计划
- 旧 visual workbench 中的 Kitty/half-block 部分
- `docs/feature-inventory.md` 第 12、13 节终端/远程能力

如果旧文档与本交接、GUI spec 冲突，以本交接和 GUI spec 为准。

## 11. Definition of Done

迁移只有同时满足以下条件才完成：

- 产品二进制没有 `server` 和 `connect`；
- Cargo 不再依赖终端 UI/graphics crates；
- 默认启动原生 GUI；
- CUDA、portable wgpu GPU、CPU 三条后端路径都有一致性测试；
- Intel 集显和至少一个非 Intel 集成 GPU 实机验证；
- CPU-only 机器可启动、编辑、Apply & Run；
- 所有 feature inventory 的非终端功能在 GUI 有可见鼠标入口；
- 多 Channel、多 Kernel、多 basis、多 Growth 完整旅程通过；
- 保存、关闭、重新打开后实验恢复；
- 候选 Release 在树莓派通过真实键鼠与视觉 agentic 测试；
- Windows、macOS、Linux x86_64/ARM64 发布物通过启动 smoke；
- 稳定版 Release 发布，不以 prerelease 代替最终交付。
