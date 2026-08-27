# Cellarium 功能与验收清单

> 对应版本：v0.2.2（工作树提交 `271b79e`）
> 文档性质：产品功能清单、状态核对表、缺陷清单。
> 本文只把代码中已经存在的能力标为“已实现”；设计稿或口头约定不会冒充成已完成。

## 1. 状态标记

| 标记 | 含义 |
| --- | --- |
| ✅ | 已有实现，且至少有自动测试或实际交互证据 |
| 🧪 | 已有实现，但仍缺完整的真实用户级 agentic 验收 |
| ⚠️ | 已实现但存在已复现缺陷，不能视为验收通过 |
| ⏳ | 已确认需求，但尚未完整实现或验证 |
| ❌ | 明确不支持、已取消，或旧文档中的过时要求 |

## 2. 产品定位和运行方式

### 2.1 单一可执行文件

- ✅ 一个 `cellarium` 二进制同时承担本地程序、远端服务器和 C/S 客户端。
- ✅ `cellarium`：在当前设备直接模拟并渲染。
- ✅ `cellarium server`：从 stdin/stdout 运行远端协议服务器。
- ✅ `cellarium connect <ssh-host>`：通过 SSH 启动远端 server，在本机渲染与交互。
- ✅ `--ssh-command <path>`：指定 SSH 程序，也可用 `CELLARIUM_SSH_COMMAND`。
- ✅ `--kernel <path>`：直接加载核定义。
- ✅ `--experiment <path>`：直接加载实验。
- ✅ `--save-experiment <path>`：把当前实验写出。
- ✅ `--version` / `-V`。
- ✅ 协议两端版本不兼容时拒绝连接，不静默误读。

### 2.2 平台和计算后端

- ✅ Linux x86_64 / ARM64 发布物。
- ✅ macOS x86_64 / ARM64 发布物。
- ✅ Windows x86_64 / ARM64 发布物。
- ✅ Linux 二进制动态检测 CUDA；有可用 NVIDIA/CUDA 时使用 GPU。
- ✅ CUDA 不可用时自动回退 CPU。
- ✅ macOS、Windows 使用 CPU 后端。
- ✅ 内置 Conway 和 Lenia/Orbium 规则，可在模拟界面切换。
- 🧪 自定义 basis、多通道、多核实验同时支持 CPU 与 CUDA；仍需要每个稳定版做 CPU/CUDA 数值一致性与真实运行验收。

## 3. 核心数据模型

### 3.1 名词

| 名词 | 含义 |
| --- | --- |
| World | 当前模拟状态，包含每个逻辑格子的各通道数值 |
| Tiling | 周期密铺几何，包括平移基向量、原型多边形、中心晶胞内实例及接缝 |
| Basis polygon | 一个晶胞内具有独立状态语义的多边形；不是颜色通道 |
| Channel | 每个 basis polygon 上的一层标量状态 |
| Binding | 一个 `(basis polygon, 输出通道)` 对 |
| RuleSet | 某个 Binding 使用的完整规则：核列表、Growth 程序、参数和更新模式 |
| Kernel | 从某个源通道采样并卷积，生成一个传给 Growth 的标量输入 |
| Growth | 根据 `self`、所有核输入和参数计算下一步 Value 或 Rate |

### 3.2 数量关系（必须以此为准）

设：

- `B` = 中心晶胞内 basis polygon 数量；
- `C_active` = 非冻结通道数量；
- `K(b,c)` = Binding `(basis=b, channel=c)` 的核数量。

则：

1. ✅ Growth / RuleSet Binding 的逻辑数量为 `B × C_active`。
2. ✅ 每个 Binding 都有自己的 Growth 程序。
3. ✅ 每个 Binding 默认有 1 个核；用户必须显式添加更多核。
4. ✅ 某个 Growth 的核参数数量是该 Binding 的 `K(b,c)`，不是全局通道数。
5. ✅ Growth 函数总参数数量是 `1 + K(b,c)`：一个 `self` 加每个核一个标量。
6. ✅ 核的 source channel 可以与输出 channel 不同，因此支持跨通道耦合。
7. ✅ 多个 Binding 初始可共享同一个 RuleSet；编辑其中一个时按 copy-on-write 分离，降低多 basis、多通道的编辑负担。

例子：

| 晶胞内多边形 | 活跃通道 | Growth Binding | 默认有效核总数 |
| ---: | ---: | ---: | ---: |
| 1 | 1 | 1 | 1 |
| 1 | 3 | 3 | 3 |
| 2 | 1 | 2 | 2 |
| 2 | 3 | 6 | 6 |

这里“默认有效核总数”按每个 Binding 一个核计算；用户给任一 Binding 添加第二个核后，总数再增加。

### 3.3 默认值

- ✅ 默认实验是单通道 Lenia/Orbium，256×256、周期边界。
- ✅ 默认通道数 1。
- ✅ 默认每个 Binding 的核数 1。
- ✅ 新增通道和新增核都需要用户显式操作。
- ✅ 没有自定义 Tiling 时，默认模拟仍是兼容的方形 RasterGrid。
- ✅ 进入 Tiling 并选择“New blank”时画布为空；不会暗中生成一个方形密铺。

## 4. 模拟主界面

### 4.1 模拟控制

- ✅ 暂停/继续：`Space` 或 `P`。
- ✅ 单步：`N` 或 `Enter`。
- ✅ 重置：`R`。
- ✅ 随机化：`A`。
- ✅ 清空：`C`。
- ✅ 切换 Conway：`1`。
- ✅ 切换 Lenia：`2`。
- ✅ 进入 Workbench：`W`。
- ✅ 退出：`Q`、`Esc` 或 `Ctrl+C`。

### 4.2 视口交互

- ✅ 左键绘制，拖动可连续绘制。
- ✅ 右键擦除，拖动可连续擦除。
- ✅ 中键拖动画布。
- ✅ 滚轮以指针位置为中心缩放。
- ✅ 鼠标检查某个格子的精确值。
- ✅ 自适应填充可用画布；缩放和平移状态保留。
- ⚠️ 这些路径历史上出现过坐标偏移、缩放闪烁、初始画面缩成小块等问题；当前代码有针对性修复，但稳定版仍需完整 agentic 回归后才能关闭风险。

### 4.3 状态与性能指标

- ✅ 当前规则、运行/暂停、tick、世界尺寸、zoom、inspect 和显示协议。
- ✅ Direct 模式显示后端 step 和 UI/render 开销。
- ✅ C/S 模式区分：
  - server simulation rate；
  - snapshot receive rate；
  - UI draw rate；
  - fresh RGBA graphics rate；
  - 可观测时的 Kitty presentation/consume rate；
  - input sequence / ack。
- ✅ 指标使用独立事件源，不再把 UI 重绘次数当作新图像帧率。

## 5. Workbench 总体交互

### 5.1 布局

- ✅ 左栏 Experiment outline：World、Tiling、Channels、Kernels、Growth、Experiment。
- ✅ 中栏 Canvas：当前 section 的主要可视化和编辑区。
- ✅ 右栏 Inspector：当前对象、状态、快捷键、诊断和语法帮助。
- ✅ 宽终端显示三栏；窄终端隐藏 Inspector，工具栏最多折成四行。
- ✅ outline 项和工具栏动作可以鼠标点击。
- ✅ `T` 或点击切换 section。
- ✅ `Tab` / `Shift+Tab` 在 Outline、Canvas、Inspector 之间切焦点。
- ✅ Inspector 内容可滚轮上下滚动。

### 5.2 草稿事务

- ✅ Workbench 同时保存 authoritative（已应用）与 draft（编辑中）实验。
- ✅ 状态：Clean、Dirty、Invalid。
- ✅ `Ctrl+Z` / `Ctrl+Y`：撤销/重做。
- ✅ `Ctrl+R`：把 draft 恢复为 authoritative。
- ✅ `Ctrl+Enter`：验证、Apply，并开始运行。
- ✅ Invalid draft 禁止 Apply，不覆盖远端有效实验。
- ✅ C/S Apply 带 base revision；远端状态冲突不会静默覆盖。
- ✅ 远端回传 authoritative 实验元数据，客户端镜像 basis、channel、RuleSet、kernel、Growth 等编辑状态。
- ✅ `W` 离开 Workbench 回到模拟；旧 Workbench graphics placement 应被删除。

## 6. World 编辑器

- ✅ 显示当前/草稿世界。
- ✅ `]` 切换编辑通道。
- ✅ `V` 切换 Composite 与选中通道视图。
- ✅ 左键绘制、右键擦除、中键平移、滚轮缩放。
- ✅ World 草稿修改可撤销/重做。
- 🧪 自定义 polygon/basis 模拟使用实际多边形几何渲染，不应重新显示成方格；需继续作为每版真实视觉验收项。

## 7. Tiling / 晶胞编辑器

### 7.1 创建入口

- ✅ 默认可从空白开始：`B` / New blank。
- ✅ 预设：
  - Square；
  - Equilateral triangles（一个晶胞两个三角形 basis）；
  - Regular hexagon；
  - Octagon + square（4.8.8 密铺，一个晶胞两个 basis）。
- ✅ `P` 循环预设。
- ✅ `D` 进入绘制形状工具。
- ✅ `A` 添加新 basis polygon。
- ✅ `N` 切换 basis。
- ✅ `+` / `-` 调整规则多边形边数（3–64）。
- ✅ `0` 把密铺适配到画布。

### 7.2 自由绘制

- ✅ 鼠标逐点绘制自定义多边形。
- ✅ 开放路径显示指针预览线。
- ✅ 点击第一个顶点、双击或 `Enter` 关闭多边形。
- ✅ `Esc` 取消当前绘制。
- ✅ 绘制期间 `Ctrl+Z` 删除刚放置的顶点，`Ctrl+Y` 恢复。
- ✅ 放置顶点时立即拒绝：
  - 与已有顶点重合；
  - 新边和已有开放路径相交/接触；
  - 非有限坐标；
  - 超过 64 个顶点。
- ✅ 闭合时验证：至少 3 点、逆时针、非零面积、无零长度边、无自交。
- 🧪 关闭动作过去有“说明写了但操作不生效”的缺陷；当前实现具备三种关闭路径，仍需每版用真实鼠标和键盘验证。

### 7.3 周期晶胞显示和选择

- ✅ 中央晶胞内的可编辑 basis polygon 强调显示。
- ✅ 周围周期邻接副本降低透明度显示，用于理解真实密铺，而不是只画横平竖直的矩形网格。
- ✅ 正六边形使用非正交平移向量；Octagon + square 显示混合多边形。
- ✅ 点击中央或周期副本都能映射回对应 basis。
- ✅ 选择、拖动顶点、右键删除、滚轮缩放、中键平移。

### 7.4 密铺辅助与约束

- ✅ 只允许完整 edge-to-edge 接缝。
- ❌ T 型接缝明确不支持；任何旧文档里“允许 T-junction”的内容已过时。
- ✅ 验证周期覆盖：gap、overlap、crossing、open seam、方向/退化问题、Euler 拓扑一致性。
- ✅ `S` Solve seams：从接近的完整边提出候选配对，联合优化顶点和平移向量，得到精确周期密铺。
- ✅ 解算后保存接缝约束；继续拖动一个受约束顶点时，相关顶点和晶格向量联动，尽量保持密铺。
- ✅ 显示求解接缝数、最大位移、残差和诊断。
- ⚠️ 求解器是“从足够接近的完整边开始”的辅助器，不是任意乱画形状的全局组合搜索器；找不到完整边对时会要求用户先把对应边摆近。
- 🧪 “用户粗摆 → 自动消除缝隙 → 联动微调”的完整体验已有算法骨架和单测，但仍缺覆盖复杂多多边形晶胞的完整 agentic 验收。

## 8. Channels / 通道编辑器

### 8.1 通道管理

- ✅ 默认一个 `state` 通道。
- ✅ `A` 添加通道。
- ✅ `Del` 删除选中通道。
- ✅ `]` 选择下一个通道。
- ✅ Inspector 中每个通道有独立可点击行。
- ✅ `F` 冻结/解冻：冻结通道不再拥有需要更新的 Growth Binding。
- ✅ `X` 显示/隐藏通道。

### 8.2 颜色和合成

- ✅ `V` 在 Composite 与单通道视图之间切换。
- ✅ `C` 循环颜色预设。
- ✅ `E` 输入精确 RGB 颜色。
- ✅ 单通道默认使用黑底高对比浅色。
- ✅ 三通道默认 RGB。
- ✅ 域内背景纯黑。
- ✅ 域外区域保留深色背景，用来区分实际模拟域。
- ✅ 颜色、可见性、opacity 是持久化实验数据。
- ✅ Channels Canvas 显示真实运行状态，而不是随机占位噪声。
- ✅ 自定义非矩形密铺使用与 Simulation 一致的 polygon scene，而不是把 256×256 栅格强行斜切。

### 8.3 已知生命周期缺陷

- ⚠️ 删除通道后再添加，当前 `WorkbenchState::add_channel` 可能按长度生成重复名称（例如第二个 `channel_3`），使草稿变为 Invalid。
- ⚠️ 上述操作后 Undo 可能让 `selected_channel` 仍指向已不存在的通道，Inspector 显示 `selected: —`。
- ⚠️ 已归一化的多 basis 规则中冻结通道时，旧 RuleSet/default/binding 清理不完整，可能留下指向 frozen channel 的规则并使草稿 Invalid。
- ⚠️ 因此 Channel 的正常新增/显示路径可用，但“删除→新增→Undo”和“冻结/解冻”目前不能算验收通过。

## 9. Kernels / 核编辑器

### 9.1 归属和路由

- ✅ Kernel 隶属于选中的 `(basis, output channel)` RuleSet。
- ✅ 默认一个核，更多核必须 `A` 显式添加。
- ✅ `Del` 删除选中核。
- ✅ `]` 切换核。
- ✅ `S` 更改 source channel。
- ✅ `U` 更改 output channel / Binding。
- ✅ 删除会导致 Growth 程序引用缺失的核时拒绝删除，不留下半坏状态。
- ✅ RuleSet 共享、local override、copy-on-write 分离和恢复默认。

### 9.2 可视化

- ✅ 栅格核和周期 polygon/basis 核都用高分辨率 graphics 可视化。
- ✅ 周期核的一个数值单元是一个 basis polygon；不会把六边形核重新画成方格。
- ✅ 画出 active、zero、inactive/support 外和空白格子的不同状态。
- ✅ 选中格、源 basis、offset、anchor 和数值在 Inspector 中显示。
- ✅ 大核支持缩放和平移，每个内部 cell 都应可到达。
- ✅ `0` 适配核到画布。

### 9.3 数值和支持域编辑

- ✅ `M` 在 Weights 与 Support 工具间切换。
- ✅ 左键/拖动绘制权重；右键设为 0。
- ✅ 选中 active cell 后滚轮调整浮点值：
  - 普通步长 ±0.05；
  - Shift ±0.005；
  - Ctrl ±0.5。
- ✅ inactive/空白位置滚轮用于缩放，不会误改值。
- ✅ `E` 或 `Enter` 打开精确浮点输入，支持提交、取消和非法值诊断。
- ✅ `R` 编辑 stencil 尺寸与 anchor。
- ✅ Support 工具控制核的形状/激活掩码，不只是修改值。

### 9.4 核预设和采样几何

- ✅ `P` 按当前 support 生成 Gaussian 权重。
- ✅ `G` 精确编辑 Gaussian sigma。
- ✅ `Q` 切换两种采样度量：
  - **Affine / LatticeAffine**：按晶格坐标采样，形状会随晶格仿射变换；
  - **World / WorldEuclidean**：按屏幕/世界中的真实 polygon 位置距离采样，在六边形等非正交密铺上保持直观的圆形/高斯形状。
- ✅ Potential 保留卷积原始加权和，不自动除以核权重总和。
- ⚠️ Kernel 页面历史上出现过点击选不中部分六边形、外圈颜色误导、空核全黑等问题；当前代码包含共同坐标变换、inactive 锁定和空状态提示，但仍需完整 agentic 回归确认。

## 10. Growth / 生长函数编辑器

### 10.1 Binding 和签名

- ✅ 编辑目标明确显示为 `basis B / channel C`。
- ✅ 完整签名显示在 Canvas 和 Inspector：

  ```text
  fn growth(self: Scalar, k1: Scalar, ..., kN: Scalar) -> Rate|Value
  ```

- ✅ `self` 是目标 basis/通道当前值。
- ✅ 每个 `kN` 是该 RuleSet 中对应 Kernel 的原始卷积结果。
- ✅ 参数（例如 `mu`、`sigma`）作为外部只读标量参与表达式。
- ✅ Kernel 数变化后签名和输入数量同步变化。

### 10.2 更新模式

- ✅ `M` 切换：
  - **Rate**：`next = clamp(self + dt × result, 0, 1)`；
  - **Value**：`next = clamp(result, 0, 1)`。
- ✅ `dt` 在 Experiment 中编辑。
- ✅ `clamp(x, lo, hi)` 表示低于 lo 取 lo，高于 hi 取 hi。
- ✅ Potential 不会在进入 Growth 前自动归一化。

### 10.3 语言

- ✅ Rust 风格表达式语言，不是完整 Rust。
- ✅ 最后一个无分号表达式是 block / program 的结果。
- ✅ `let name = expression;`。
- ✅ `if condition { expression } else { expression }`，else 必须存在。
- ✅ 数字、`true`、`false`、`pi`、`e`。
- ✅ 单行 `// comment`。
- ✅ 运算：`+`、`-`、`*`、`/`、`^`、`!`。
- ✅ 比较：`==`、`!=`、`<`、`<=`、`>`、`>=`。
- ✅ 逻辑：`&&`、`||`。
- ✅ 内置函数：
  - `sqrt(x)`、`abs(x)`、`exp(x)`、`log(x)`；
  - `sin(x)`、`cos(x)`、`tanh(x)`；
  - `floor(x)`、`ceil(x)`、`round(x)`、`sign(x)`；
  - `min(a,b)`、`max(a,b)`、`step(edge,x)`；
  - `clamp(x,lo,hi)`、`smoothstep(lo,hi,x)`；
  - `mix(a,b,t)`、`gauss(x,mu,sigma)`。
- ❌ 当前语言没有 `return`、循环、可变变量或副作用；分支值由分支最后表达式产生。
- ✅ 类型检查、未知变量/函数、参数数量、条件类型和结果类型诊断。
- ✅ 数值区间危险分析（例如潜在非有限值）。

### 10.4 文本编辑体验

- ✅ `E` 开始/结束源码编辑，`Esc` 完成。
- ✅ 多行编辑、可见光标、选择高亮。
- ✅ 方向键、Home/End、按词移动。
- ✅ Backspace/Delete、换行。
- ✅ Shift 扩展选择。
- ✅ `Ctrl+A` 全选，`Ctrl+U` 删除到行首。
- ✅ 每次编辑实时重新解析、类型检查和刷新诊断。
- ✅ 右侧 Inspector 提供可滚动的语法、内置函数、签名、模式、变量含义和参数帮助。

### 10.5 精细 graphics 图

- ✅ 0 或 1 个核输入：精细像素曲线图。
- ✅ 2 个及以上核输入：以前两个核为 x/y 轴的 2D heatmap，其余参数固定。
- ✅ 图中显示坐标轴、范围、曲线/颜色结果和零值参考。
- ✅ `d` / `D` 精确编辑 plot min/max。
- ✅ 默认 plot domain 根据核权重和输入范围推导，不固定死为 [0,1]。
- ✅ 非法程序保留最后一张有效图，但明确标记 stale，并显示源码 span 诊断。
- ⚠️ Growth 图历史上出现过“整条平线/空图”问题；等值判断如 `potential == 2/6` 本身也只在精确采样点命中，图必须呈现孤立阈值标记而不能静默看似恒零。此项仍是稳定版 agentic 必测项。

## 11. Experiment / 应用、运行和持久化

### 11.1 实验检查与运行

- ✅ 汇总世界尺寸、basis 数、通道数、RuleSet/Binding 数、有效核总数、Growth 数、dt、seed 和诊断。
- ✅ `D` 精确编辑模拟 dt。
- ✅ `Ctrl+Enter` 是 **Apply & Run**，不是只保存：
  1. 验证整个 draft；
  2. 编译 topology/RuleSet/Growth；
  3. 发送远端 Apply（C/S）或替换本地 backend；
  4. 清空暂停状态并开始运行；
  5. 收到新 revision/ack 后把 draft 标为 Clean。
- ✅ Apply 失败保持原 authoritative 实验可运行。

### 11.2 默认持久化

- ✅ 数据目录：
  - 若 `XDG_DATA_HOME` 是绝对路径：`$XDG_DATA_HOME/cellarium/`；
  - 否则：`$HOME/.local/share/cellarium/`。
- ✅ `workbench.ron`：active、draft、active revision、base revision。
- ✅ `experiment.ron`：可独立加载/运行的实验。
- ✅ `Ctrl+S` 保存 active/workspace。
- ✅ `Ctrl+E` 导出 draft。
- ✅ `Ctrl+L` 加载 draft。
- ✅ 定时自动保存 Workbench。
- ✅ 原子写入：临时文件、sync、rename；Unix 下新文件权限 0600。
- ✅ RON 文件带格式版本；未知新版本拒绝读取。
- ✅ 旧格式实验有受控迁移边界，不静默误解新字段。

## 12. 图形、终端与降级

- ✅ 支持 Kitty graphics、Sixel、iTerm2 graphics 和 half-block。
- ✅ 检测到 Kitty 或其它受支持图形协议时默认使用 graphics。
- ✅ Kitty 本地 Unix 优先共享内存传帧；失败时回退 inline graphics。
- ✅ graphics 全部不可用时回退 half-block。
- ✅ C/S 模式在本机完成高分辨率渲染，服务器只做模拟并发送逻辑快照。
- ✅ 最新帧优先队列：积压时丢弃过时中间帧，输入不等待旧图像编码。
- ✅ Workbench section 切换、resize、离开、协议降级和退出时删除旧 Kitty placement。
- ✅ half-block 与 graphics 使用同一个控制器和逻辑坐标变换，降级不应失去鼠标/键盘交互。
- 🧪 Direct `kitten ssh` 仍支持高精度 graphics，但性能和交互延迟要与 C/S 独立测量。

## 13. 远端 C/S

- ✅ SSH 子进程通过 stdin/stdout 承载版本化二进制协议。
- ✅ server 负责 GPU/CPU step；client 负责终端 UI、graphics 和输入。
- ✅ latest-only snapshot，避免网络抖动堆积旧状态。
- ✅ 每条输入带 sequence；server 回传 `applied_input_seq`，可测真实端到端输入 ack。
- ✅ Apply 带 revision；远端返回 authoritative ExperimentSpec 和选中编辑元数据。
- ✅ 客户端本地乐观反馈与 server ack 分开统计。
- ✅ 断开、退出和测试清理应只终止本次会话进程，不残留多 server、Kitty image 或共享内存对象。

## 14. 测试与发布门禁

### 14.1 自动测试

- ✅ Rust 单元测试覆盖模型、parser/typecheck、kernel、tiling、solver、渲染变换、历史和协议。
- ✅ CPU/CUDA 后端路径有测试。
- ✅ PTY E2E 覆盖协议、键鼠字节、Apply ack、Kitty 命令消费和 half-block。
- ✅ GitHub Actions 构建多 OS/架构单一二进制并发布 SHA256SUMS。

### 14.2 Agentic 用户级测试

- ✅ 已有真实测试框架：Xvfb → Openbox → Kitty → 发布版 ARM64 client → tinker server。
- ✅ Agent 必须看真实 framebuffer，依据最新截图选择坐标，发送真实 X11 键鼠事件，再从视觉上判断结果。
- ✅ 每个动作要求 before/after PNG 和语义观察，不允许只看静态测试或 hash 就算通过。
- ✅ Kitty 与 half-block 都必须走关键旅程。
- ✅ 本机树莓派只运行预编译 Release 客户端，不本地构建；GPU/性能计算放在 tinker。
- ⚠️ v0.2.2 最近的 Channel/Growth 数量 agentic 旅程复现了本文件第 8.3 节缺陷，所以当前整体结论不是 PASS。

## 15. 当前必须修复的已知问题

按优先级列出当前仍未关闭的问题：

1. ⚠️ **通道删除后再新增会产生重复名称并导致 Invalid。**
2. ⚠️ **上述路径 Undo 后 selected channel 可能悬空。**
3. ⚠️ **冻结归一化多 basis 通道时 RuleSet/binding 清理不完整。**
4. ⚠️ **Inspector 的数量口径不够清楚。** 当前把全局 `channels: N` 与当前 Binding 的 `kernels: K` 放在一起，用户容易误以为两者应相等。应明确显示：
   - basis polygons；
   - active/frozen channels；
   - Growth bindings = B × C_active；
   - 当前 Binding kernel count；
   - 全部 Binding effective kernel count。
5. 🧪 **完整稳定版 agentic 回归尚未重新通过。** 需要覆盖绘制/闭合三角形、六边形 Apply 后真实几何、RGB Channels、Kernel 支持域/浮点/精确输入、Growth 曲线/热图、Apply & Run、resize、退出清图和 half-block。

## 16. 已确认但仍需继续打磨的产品能力

- ⏳ 更强的全局密铺辅助：当用户画出的多边形离可密铺状态较远时，给出更直观的候选边对应和可操作修复建议，而不仅是报错。
- ⏳ 对复杂多多边形晶胞，完善“粗摆 → 求解 → 联动顶点微调”的可发现性、冲突解释和失败恢复。
- ⏳ 给 RuleSet 的共享、local override、reset-to-default 增加更直接的可视化控制，避免用户必须从 Inspector 文本推断。
- ⏳ Kernel active/inactive/support/zero 的图例和当前工具状态需要更醒目。
- ⏳ Growth 多于两个核时，目前图只用前两个做 heatmap，其余固定；需要更明确的 pinned-input UI。

## 17. 已取消或过时的内容

- ❌ 不允许 T-junction；旧的 `tests/agentic/full-journey.md` J09 和早期设计文档仍写“允许 T”，应更新。
- ❌ “Tiling 默认已有一个方形 polygon”是旧行为；当前产品要求是默认空白，用户选择预设或从头绘制。
- ❌ 不要求 Growth 显式 `return`；最终表达式作为结果，因而 `if/else` 可以自然产生分支值。
- ❌ Potential 不自动归一化；保留核卷积的原始值。
- ❌ 不把 Channel 数与 Kernel 数强行绑定。
- ❌ 不使用字符画作为正式 Kernel/Growth 可视化；graphics 是首选，half-block 只负责可交互降级。

## 18. 用户审阅区

请按下面几类检查是否有遗漏：

- [ ] 运行方式和平台
- [ ] 模拟主界面操作
- [ ] World 编辑
- [ ] 晶胞/密铺创建、验证、求解和联动编辑
- [ ] basis polygon 与 Channel 的语义
- [ ] Channel 数量、颜色、显示和冻结
- [ ] RuleSet 共享/分离策略
- [ ] Kernel 数量、路由、形状、support、数值和预设
- [ ] Growth 签名、语法、Rate/Value 和精细图
- [ ] Apply & Run
- [ ] 保存、自动保存、加载和格式迁移
- [ ] Direct / C/S / Kitty / half-block
- [ ] 性能指标、进程清理和测试门禁

如果某一项的产品语义与你的预期不同，应先修改本清单，再改实现和 agentic 旅程；这样后续不会再次出现“测试通过了，但测的不是用户真正要的功能”。
