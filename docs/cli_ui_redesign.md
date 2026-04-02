# 基于 prompt_toolkit + Typer + Rich 的终端 CLI 界面优化设计

## 1. 目标

为当前项目设计一套更易用、可扩展、同时兼容交互/非交互两种使用方式的终端 CLI 界面方案。

本方案只提供设计，不改动现有业务逻辑。

核心目标：

1. 用 `prompt_toolkit` 提升交互体验：输入、补全、确认、选择、快捷键。
2. 用 `Typer` 规范命令行入口：支持子命令、参数解析、`--help`、脚本化调用。
3. 用 `Rich` 优化输出渲染：状态信息、表格、步骤面板、进度条、错误高亮。
4. 尽量复用现有 `UserInterface` 抽象，降低对 `main.py`、`agent/workflow.py`、`agent/solve_agent.py` 的侵入。

---

## 2. 当前问题

从现有实现看，CLI 交互主要依赖 `print()` + `input()`：

- `utils/user_interface.py:86` 的 `CommandLineInterface` 直接使用标准输入输出。
- `main.py:101` 直接实例化 `CommandLineInterface()`。
- `agent/solve_agent.py:105`、`utils/tools.py:169` 等位置会持续输出步骤与工具执行信息。

当前方式的问题：

1. **交互能力弱**
   - 只有纯文本输入，缺少补全、默认值、选择器、快捷键。
   - 手动审批步骤只能输入数字或 `y/n`，交互效率低。

2. **展示层次不清晰**
   - 思考、工具调用、工具输出、错误提示全部混在普通文本里。
   - 长输出缺少颜色区分、边界、状态标识。

3. **缺少标准 CLI 入口结构**
   - 当前入口偏“直接运行程序”，不适合扩展成 `solve`、`resume`、`config`、`tools` 等子命令。
   - 非交互场景下无法自然支持 `--help`、脚本调用、CI 集成。

4. **交互逻辑与渲染逻辑耦合**
   - 业务代码只知道 `display_message()`、`confirm_resume()` 等接口，这很好。
   - 但具体实现只有一种 `print/input` 版本，无法灵活切换到更强的终端交互层。

---

## 3. 设计原则

1. **业务逻辑不重写，先替换界面层**
   - 保留 `UserInterface` 抽象接口。
   - 新增更强的 CLI 实现类，而不是把交互代码散落到业务流程里。

2. **命令入口与交互会话分层**
   - `Typer` 负责命令结构与参数解析。
   - `prompt_toolkit` 负责进入命令后的交互细节。
   - `Rich` 负责所有输出渲染。

3. **交互优先，但必须保留非交互模式**
   - 用户可以直接运行交互式命令。
   - 也可以通过命令参数一次性传入题目、模式、是否恢复等，适合自动化使用。

4. **渐进迁移**
   - 第一阶段先兼容现有 `Workflow` / `SolveAgent`。
   - 后续再考虑更高级的 TUI 化能力，如分栏布局、实时日志面板。

---

## 4. 框架职责划分

### 4.1 prompt_toolkit：交互输入层

负责：

1. `PromptSession` 输入框
2. 自动补全
3. 单选/多选
4. `y/n` 确认
5. 手动审批时的快捷键
6. 历史记录与快捷编辑

适合承接的现有接口：

- `select_mode()`
- `input_question_ready()`
- `confirm_flag()`
- `confirm_resume()`
- `manual_approval()`
- `manual_approval_step()`

### 4.2 Typer：命令入口与子命令结构

负责：

1. CLI 根命令
2. 子命令组织
3. 参数解析
4. `--help` 文档
5. 非交互模式支持
6. 命令参数校验

建议优先选择 `Typer`，原因：

1. API 更现代，基于类型注解，可读性更好。
2. 后续命令扩展成本低。
3. 仍然建立在 `Click` 之上，生态成熟。

如果后续追求更底层控制，也可以退回 `Click`，但当前设计推荐 `Typer`。

### 4.3 Rich：输出渲染层

负责：

1. 彩色日志
2. 表格展示
3. `Panel` / `Rule` / `Tree` 组织信息层级
4. 进度条与状态提示
5. 错误高亮
6. 差异对比风格展示

适合承接的现有输出场景：

- 启动欢迎页
- 当前模式说明
- 工具调用列表
- 每一步思考状态
- 工具执行结果摘要
- 候选 flag 展示
- 失败/终止/恢复存档提示

---

## 5. 总体架构

建议新增一层 `cli` 包，将“命令入口 / 交互适配 / 渲染输出”从当前业务代码中拆分出来。

### 5.1 推荐目录结构

```text
cli/
├── app.py                     # Typer 应用入口
├── commands/
│   ├── solve.py               # solve 子命令
│   ├── checkpoint.py          # resume/list/clear 等
│   ├── config.py              # 配置检查/模板展示
│   └── tools.py               # 工具列表/测试命令
├── ui/
│   ├── interface.py           # Rich + prompt_toolkit 的 UI 实现
│   ├── prompts.py             # 输入/确认/选择组件
│   ├── render.py              # Rich 渲染工具
│   ├── theme.py               # 颜色与样式主题
│   └── widgets.py             # 表格、面板、审批视图等复用组件
└── adapters/
    └── workflow_runner.py     # 将命令参数转换为 Workflow 调用
```

### 5.2 分层关系

```text
Typer 命令层
    ↓
CLI 适配层（参数 → Workflow 调用）
    ↓
Rich + prompt_toolkit UI 实现（实现 UserInterface）
    ↓
Workflow / SolveAgent / ToolUtils 现有业务层
```

关键点：

- `Workflow` 和 `SolveAgent` 继续依赖 `UserInterface` 抽象。
- 新界面只需实现同一套接口，即可替换 `CommandLineInterface`。
- `main.py` 后续可以变成对 `cli.app` 的轻量封装，或直接由 Typer 接管入口。

---

## 6. 命令设计

## 6.1 根命令

建议统一成一个根命令，例如：

```bash
buuctf-agent --help
```

### 6.2 子命令结构

```bash
buuctf-agent solve [OPTIONS]
buuctf-agent resume [OPTIONS]
buuctf-agent checkpoint list
buuctf-agent checkpoint clear
buuctf-agent config check
buuctf-agent tools list
```

### 6.3 `solve` 子命令

建议支持两类用法。

#### 交互式

```bash
buuctf-agent solve
```

行为：

1. 若检测到存档，弹出恢复确认。
2. 引导用户选择自动/手动模式。
3. 引导确认题目已写入 `question.txt` 或直接粘贴题目。
4. 显示求解过程。

#### 非交互式

```bash
buuctf-agent solve --question-file question.txt --auto --no-resume
```

或：

```bash
buuctf-agent solve --question "题目内容" --manual
```

建议参数：

- `--question-file PATH`：从文件读取题目。
- `--question TEXT`：直接从命令行传题目文本。
- `--auto/--manual`：指定模式。
- `--resume/--no-resume`：是否尝试恢复存档。
- `--attachments-dir PATH`：覆盖附件目录。
- `--show-think`：是否显示详细思考文本。
- `--plain`：关闭 Rich 彩色输出，便于日志重定向。

### 6.4 `resume` 子命令

```bash
buuctf-agent resume
```

行为：

1. 自动检测最近存档。
2. 展示可恢复条目。
3. 让用户选择要恢复的条目。

### 6.5 `tools list` 子命令

```bash
buuctf-agent tools list
```

用 `Rich Table` 展示：

- 工具名
- 描述
- 参数摘要
- 是否启用

### 6.6 `config check` 子命令

```bash
buuctf-agent config check
```

用 `Rich Panel` + `Table` 展示：

- 配置文件位置
- 关键配置是否完整
- 缺失字段
- 建议修复项

---

## 7. 交互设计

## 7.1 启动页

使用 `Rich Panel` 显示：

- 项目名
- 当前运行模式
- 当前模型/配置概览
- 题目来源
- 存档状态

示意：

```text
┌─ BUUCTF Agent ───────────────────────┐
│ 模式: 手动                           │
│ 题目来源: question.txt               │
│ 附件目录: ./attachments              │
│ 存档: 检测到 1 条未完成记录          │
└──────────────────────────────────────┘
```

## 7.2 模式选择

现有 `select_mode()` 为纯数字输入，建议改成 `prompt_toolkit` 单选选择。

选项：

- 自动模式：自动执行全部步骤
- 手动模式：逐步审批

增强点：

1. 默认高亮推荐项。
2. 显示每个模式的简短说明。
3. 支持方向键选择、回车确认。

## 7.3 存档恢复

现有 `confirm_resume()` 只支持 `y/n`。

建议改为：

- 若只有一条存档：显示摘要并确认是否恢复。
- 若有多条存档：用列表选择具体恢复项。

展示信息：

- 题目摘要
- 最近步骤编号
- 上次更新时间
- 模式（自动/手动）

## 7.4 题目输入

建议支持三种来源：

1. 文件：`question.txt`
2. 命令参数：`--question`
3. 交互粘贴：多行输入

其中交互粘贴可由 `prompt_toolkit` 支持：

- 支持多行编辑
- 支持历史记录
- 支持快捷键提交

## 7.5 手动审批

这是最值得优化的交互点，对应当前 `manual_approval_step()`。

建议把单步审批拆成三个显示块：

1. **思考摘要**：本步目标、为什么这样做。
2. **工具调用表格**：工具名、参数摘要。
3. **审批动作栏**：批准 / 反馈 / 终止。

建议快捷键：

- `Enter`：批准执行
- `f`：输入反馈并重试
- `q`：终止解题
- `v`：展开/折叠详细参数
- `c`：复制当前命令文本（后续可选）

Rich 展示示意：

```text
┌─ 第 4 步审批 ─────────────────────────┐
│ 目标: 检查附件中的可执行文件行为      │
├──────────────────────────────────────┤
│ 工具 1  execute_shell_command         │
│ 参数    file ./attachments/a.out      │
├──────────────────────────────────────┤
│ [Enter] 批准  [f] 反馈  [q] 终止      │
└──────────────────────────────────────┘
```

## 7.6 flag 确认

现有 `confirm_flag()` 仅用普通文本展示。

建议：

- 用 `Panel` 高亮展示候选 flag。
- 成功时用绿色，待确认时用黄色。
- 支持快捷确认键：`y/n`。

---

## 8. 输出渲染设计

## 8.1 消息等级

建议统一消息等级，而不是所有内容都直接 `print()`：

- `info`：普通状态
- `success`：阶段成功 / 找到 flag
- `warning`：可疑情况 / 用户需要关注
- `error`：执行失败
- `debug`：详细思考与工具原始输出

可在 `RichRenderer` 中提供：

- `render_info(message)`
- `render_success(message)`
- `render_warning(message)`
- `render_error(message)`
- `render_step_header(step_no)`

## 8.2 步骤展示

对应当前 `agent/solve_agent.py:105` 的“正在思考第 N 步...”，建议增强为：

1. 顶部 `Rule` 分隔线
2. 步骤编号 + 当前状态徽标
3. 若有耗时操作，显示 `Spinner`

示意：

```text
──── 第 3 步 · 正在分析工具输出 ────
```

## 8.3 工具调用展示

当前工具执行提示来自 `utils/tools.py:169`，仅输出“执行工具 x/y”。

建议改成 `Rich Table`：

| 序号 | 工具名 | 参数摘要 | 状态 |
|---|---|---|---|
| 1 | execute_shell_command | `ls ./attachments` | running |

执行完成后刷新状态为 `done` / `failed`。

## 8.4 工具输出展示

根据输出类型分层：

1. **短文本输出**：直接放进 `Panel`。
2. **长文本输出**：默认折叠，只展示摘要和前若干行。
3. **错误输出**：红色高亮。
4. **命令差异/变更信息**：用 Rich 的 diff 风格文本展示。

可选策略：

- 默认只显示摘要。
- 加 `--verbose` 时显示完整原始输出。
- 手动审批模式可按键展开最近一次工具输出。

## 8.5 进度与状态

适合用 `Rich Progress` 或 `Status` 的场景：

- 题目预处理
- 调用模型等待中
- 工具执行中
- 存档保存中

注意：

当前步骤数本身是动态的，不适合做“总量确定”的进度条，更适合：

- Spinner
- 当前步骤号
- 最近一次动作说明

---

## 9. 与现有代码的衔接方案

## 9.1 保留 `UserInterface` 抽象

现有 `utils/user_interface.py:10` 已定义良好的抽象接口，这是本次改造的最佳接入点。

建议新增：

```python
class RichPromptToolkitInterface(UserInterface):
    ...
```

由它实现全部现有方法：

- `confirm_flag`
- `select_mode`
- `input_question_ready`
- `display_message`
- `manual_approval`
- `manual_approval_step`
- `confirm_resume`

这样：

- `Workflow` 无需知道底层是 `print/input` 还是 Rich/prompt_toolkit。
- `SolveAgent` 无需改审批逻辑主干。
- 业务层与显示层仍然解耦。

## 9.2 `display_message()` 的增强

当前 `display_message()` 只有字符串参数，功能偏弱。

设计上建议先兼容原签名：

```python
def display_message(self, message: str) -> None
```

内部可做轻量规则识别：

- 包含“错误” → error 样式
- 包含“警告” → warning 样式
- 包含“正在” → status/info 样式

后续如需要，再扩展为：

```python
def display_message(self, message: str, level: str = "info") -> None
```

但第一阶段不建议强改所有调用点。

## 9.3 `main.py` 的改造方向

当前 `main.py:92` 仍是传统入口函数。

建议最终演进为：

1. `main.py` 只保留：`app()` 或 `run()` 入口。
2. CLI 实际参数解析交给 `Typer`。
3. 原来 `main()` 中的流程拆给 `solve` 子命令。

即：

```text
main.py -> cli/app.py -> cli/commands/solve.py -> Workflow
```

## 9.4 `ToolUtils.execute_tools()` 的配合

当前 `utils/tools.py:169` 支持 `display_message()` 回调，这是很好的挂点。

可继续保留，只是把消息从普通文本改成 Rich 风格输出。

如果后续想进一步提升体验，可新增回调事件，而不仅是字符串：

- 工具开始执行
- 工具执行成功
- 工具执行失败
- 输出已截断

但第一阶段先不要求改动工具执行流程。

---

## 10. 推荐实现路径

## 10.1 第一阶段：低侵入替换 UI 实现

目标：不重写核心流程，只替换界面层。

工作内容：

1. 新增 `Typer` CLI 入口。
2. 新增 `RichPromptToolkitInterface`。
3. 用 Rich 重写消息显示。
4. 用 prompt_toolkit 重写模式选择、确认、审批。
5. `solve` 子命令里创建新的 UI 实现并注入 `Workflow`。

收益：

- 改动小。
- 风险低。
- 很快能看到界面提升。

## 10.2 第二阶段：命令体系完善

补充子命令：

1. `resume`
2. `checkpoint list/clear`
3. `tools list`
4. `config check`

收益：

- 从“一个脚本”演进为“标准 CLI 工具”。
- 更适合长期维护。

## 10.3 第三阶段：半 TUI 化增强

可选增强：

1. 分栏布局显示步骤、工具和输出。
2. 实时刷新最近一步执行状态。
3. 提供日志面板和审批面板并行视图。

这一阶段仍可主要基于 Rich 实现，不必一开始就做完整终端应用。

---

## 11. 依赖建议

建议新增依赖：

```text
typer
rich
prompt_toolkit
```

其中：

- `Typer` 负责命令结构。
- `Rich` 负责渲染。
- `prompt_toolkit` 负责交互体验。

可选依赖：

```text
shellingham
```

用途：

- 获取 shell 环境信息
- 某些情况下辅助终端兼容性判断

但不是第一阶段必须项。

---

## 12. 示例交互流程

### 12.1 交互式求解

```bash
buuctf-agent solve
```

流程：

1. Rich 欢迎页显示配置摘要。
2. 若有存档，prompt_toolkit 弹出恢复选择。
3. 选择自动/手动模式。
4. 确认题目来源。
5. 显示每一步思考状态。
6. 手动模式下对工具调用进行审批。
7. 发现 flag 后高亮展示并确认。

### 12.2 非交互式求解

```bash
buuctf-agent solve --question-file question.txt --auto --no-resume --plain
```

流程：

1. Typer 解析参数。
2. 不进入交互选择。
3. 直接构造运行参数。
4. 输出使用简化样式，适合日志采集。

---

## 13. 风险与取舍

### 13.1 风险：引入三套框架后复杂度上升

应对：

- 每个框架只负责一层，不混用职责。
- 不要让业务代码直接调用 Rich/prompt_toolkit。
- 统一通过 `UserInterface` 和 CLI 适配层接入。

### 13.2 风险：Windows 终端兼容性

`prompt_toolkit` 和 `Rich` 通常兼容性较好，但不同终端下表现可能不同。

应对：

- 提供 `--plain` 模式。
- 对不支持的样式自动降级。
- 避免第一阶段使用过重的全屏 TUI 技术。

### 13.3 风险：日志与彩色输出混杂

应对：

- 标准输出给用户看。
- 详细日志继续走现有 logging 文件。
- Rich 主要负责用户可读输出，不替代日志系统。

### 13.4 风险：现有字符串消息语义过弱

当前很多地方只传递简单文本，难以精细渲染。

应对：

- 第一阶段做字符串规则增强。
- 第二阶段再考虑事件化消息模型。

---

## 14. 最终建议

推荐采用下面的组合方案：

1. **Typer 作为统一 CLI 入口**
   - 建立 `solve / resume / checkpoint / config / tools` 子命令结构。
   - 保证非交互模式下也能正常使用 `--help` 和参数调用。

2. **prompt_toolkit 作为交互层**
   - 接管模式选择、恢复确认、手动审批、题目输入等高频交互。
   - 提供快捷键、补全、单选列表与更自然的确认体验。

3. **Rich 作为展示层**
   - 接管所有终端输出渲染。
   - 对步骤、工具调用、状态、错误、flag 做结构化展示。

4. **保留 `UserInterface` 抽象作为集成边界**
   - 这是当前代码最适合演进的接入点。
   - 可在不重写 `Workflow` 与 `SolveAgent` 的前提下完成大部分 CLI 升级。

---

## 15. 建议的落地优先级

### P0

1. 引入 `Typer` 根命令与 `solve` 子命令。
2. 新增 `RichPromptToolkitInterface`。
3. 替换模式选择、恢复确认、手动审批、flag 确认。
4. 用 Rich 重写普通消息显示。

### P1

1. 增加 `resume / checkpoint / tools / config` 子命令。
2. 增加 `--plain / --verbose / --show-think` 等运行参数。
3. 增强工具调用表格和状态展示。

### P2

1. 增加分栏显示与半实时刷新。
2. 将字符串消息逐步演进为结构化事件。
3. 做更完整的主题系统与终端兼容降级策略。

---

## 16. 结论

这次 CLI 界面优化，最合适的切入方式不是直接重写主流程，而是：

- 用 `Typer` 重构命令入口；
- 用 `prompt_toolkit` 替换低效的 `input()/print()` 交互；
- 用 `Rich` 重构终端展示；
- 用现有 `UserInterface` 作为业务层与界面层之间的稳定边界。

这样可以在保持当前架构基本稳定的前提下，显著提升：

1. 交互效率
2. 输出可读性
3. 子命令扩展能力
4. 非交互自动化能力
5. 长期维护性
