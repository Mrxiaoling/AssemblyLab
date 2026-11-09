# ARCHITECTURE — 系统架构

> Phase 0 设计文档 · 状态:待确认
> 上游需求:README 第十节核心架构图、第六节实验引擎、第九节 AI 助教。

## 1. 系统总览

```
┌────────────────────────────── 浏览器(Vue 3 + TS) ──────────────────────────────┐
│  学习首页  课程页  实验工作台  协程专题  习题中心  知识图谱  设置                  │
│  ┌──────────────────── 实验工作台(可调面板) ─────────────────────┐             │
│  │ 源码/汇编编辑 │ 汇编/反汇编 │ 寄存器 │ 标志 │ 内存 │ 栈帧 │ 轨迹 │ 解释卡 │   │
│  └────────────────────────────────────────────────────────────────┘             │
└───────────────△───────────────────────────────────────────────△────────────────┘
          REST/JSON(HTTP, localhost)                    OpenAI-compatible(可选)
                │                                                │
┌───────────────┴──────────────── Rust 后端(Axum + Tokio) ───────┴───────────────┐
│ api/        HTTP 层:路由、DTO(utoipa)、校验                                     │
│ core/       领域层:Session、StepReport、Trace、Judge、Progress                  │
│ engine/     实验引擎编排:load → step → diff → explain                          │
│ adapters/   五类可插拔适配器(见 §3)                                            │
│   ├── isa/        IsaRegistry(ISA 数据包加载,见 ISA_DESIGN.md)                 │
│   ├── emulator/   UnicornAdapter(模式 A)│ (未来)自研解释器 Adapter            │
│   ├── compiler/   NasmAdapter / ClangAdapter / GccAdapter                      │
│   ├── debugger/   GdbMiAdapter(模式 B,Phase 2)                               │
│   └── disasm/     IcedAdapter(后续加 CapstoneAdapter)                          │
│ ai/         AiTutor(OpenAI-compatible,可禁用)                                  │
│ storage/    SQLite(进度/习题记录)+ data/ 课程文件加载                          │
│ sandbox/    进程沙箱(模式 B 用,见 SECURITY_DESIGN.md)                         │
└──────────△──────────────────────────────△──────────────────────────△───────────┘
     unicorn.dll(运行时加载)      外部工具进程(nasm/clang/gdb)      data/*.toml
```

## 2. 后端工程布局(cargo workspace)

```
assembly-lab/
├── crates/
│   ├── lab-api/        # axum 路由 + DTO + OpenAPI 输出
│   ├── lab-core/       # 领域类型与业务逻辑(不依赖 HTTP/模拟器)
│   ├── lab-engine/     # 会话编排、step/diff/explain 管线
│   ├── lab-isa/        # ISA 数据包解析(serde 加载 data/isas/*)
│   ├── lab-adapter-unicorn/   # 模式 A 引擎实现
│   ├── lab-adapter-tools/     # 汇编器/反汇编器外部进程封装
│   ├── lab-adapter-gdb/       # 模式 B(Phase 2)
│   ├── lab-judge/      # 习题判题(纯函数 + 引擎调用)
│   ├── lab-ai/         # AI 助教适配(OpenAI-compatible)
│   └── lab-storage/    # rusqlite 迁移与访问
├── data/
│   ├── isas/x86-64/…           # ISA 数据包(ISA_DESIGN.md §2)
│   ├── courses/…               # 课程(lesson.toml + content.md + experiments)
│   ├── exercises/…             # 习题库
│   └── knowledge-graph.toml    # 知识图谱
├── web/                # Vue 3 前端(openapi-typescript 生成的类型在 src/api/)
├── spikes/             # 技术验证(非产品代码)
└── docs/
```

依赖方向:`api → engine → core`,adapters 实现 `core` 中的 trait,`isa` 与 `judge` 是 engine 的输入。禁止反向依赖;`core` 不出现任何具体 ISA 的名字。

## 3. 适配器接口(核心 trait,示意签名)

```rust
// lab-core/src/emulator.rs —— 模式 A 的抽象,ISA 中立
pub trait EmulatorAdapter: Send {
    /// 按 ISA 数据包与程序(已汇编的机器码)装载一个会话
    fn load(&mut self, prog: &LoadedProgram, isa: &IsaDefinition) -> Result<()>;
    /// 执行 n 条指令;返回每步的快照对与事件(由引擎计算 diff)
    fn run_steps(&mut self, n: u32, ctrl: RunControl) -> Result<Vec<StepEvents>>;
    /// 运行直到断点/终止/上限
    fn run(&mut self, breakpoints: &[u64], ctrl: RunControl) -> Result<Vec<StepEvents>>;
    /// 宿主侧修改状态(教学功能:改寄存器/改内存)
    fn set_register(&mut self, reg: RegisterId, value: RegValue) -> Result<()>;
    fn write_memory(&mut self, addr: u64, bytes: &[u8]) -> Result<()>;
    fn snapshot(&mut self) -> Result<MachineSnapshot>;
    fn reset(&mut self) -> Result<()>;
}

// 每步的原始事件(引擎层负责变成 StepReport)
pub struct StepEvents {
    pub before: MachineSnapshot,
    pub after: MachineSnapshot,
    pub mem_writes: Vec<MemWriteEvent>,
    pub exit: Option<ExitReason>,   // 正常结束 / 断点 / 上限 / 错误
}

// lab-adapter-tools —— 外部工具进程(统一超时与输出捕获)
pub trait ToolAdapter {
    fn assemble(&self, req: AssembleRequest) -> Result<AssembleOutput>;  // nasm/clang -f …
    fn disassemble(&self, req: DisasmRequest) -> Result<Vec<DisasmLine>>; // objdump/iced
}

// lab-adapter-gdb(Phase 2)—— 模式 B 的抽象
pub trait DebuggerAdapter: Send {
    fn spawn(&mut self, cfg: DebugTarget) -> Result<()>;
    fn interrupt(&mut self) -> Result<()>;
    fn step_instruction(&mut self) -> Result<StepEvents>;
    fn set_breakpoint(&mut self, file_line: SourceLoc) -> Result<BreakpointId>;
    fn read_regs(&mut self) -> Result<MachineSnapshot>;
    fn read_memory(&mut self, addr: u64, len: u32) -> Result<Vec<u8>>;
    fn stack_frames(&mut self) -> Result<Vec<StackFrame>>;
}
```

要点:

- `StepEvents` 只含**原始事实**;`StepReport`(带 diff 与解释)由 `lab-engine` 结合 ISA 元数据计算——保证「解释可以错,执行不会错」的分层(ISA_DESIGN.md D-ISA-4)。
- Unicorn 句柄非线程安全 → `UnicornAdapter` 以 **actor 线程**运行,`lab-engine` 通过命令通道交互,axum 层不直接触碰模拟器。

## 4. 一次「单步执行」的完整数据流

```
UI(点击「单步」)
 → POST /api/sessions/{id}/step  {count: 1}
 → lab-api 校验 + 会话查找
 → lab-engine 向 EmulatorActor 发 Cmd::Step{1}
 → UnicornAdapter: snapshot(before) → uc_emu_start(rip, end, 0, 1) → snapshot(after)
                    收集 mem-hook 写入事件、code-hook 命中
 → lab-engine: 用 IsaRegistry 把寄存器 id 翻译成名称/别名 → 计算 reg/flags/memory diff
              → 查 instruction-notes 得到教学解释条目
              → 记录 TraceEntry(环形缓冲)
 → 返回 StepReport JSON
 → UI: 高亮当前反汇编行、刷新寄存器/标志/内存面板、渲染 diff 解释卡、追加轨迹
```

`StepReport` 的 UI 展示契约(README 第三节要求逐项满足):

```ts
interface StepReport {
  stepNo: number;
  address: number;            // 当前指令地址
  machineBytes: [number];     // 机器码字节
  disasm: string;             // 反汇编文本
  regDiff: RegDiff[];         // 执行前后寄存器状态对
  flagDiff: FlagDiff[];       // 哪些标志位变化(真实执行结果)
  memChanges: MemChange[];    // 哪些地址变化(真实写入)
  pcJump: { from: number; to: number; kind: "seq" | "branch" | "call" | "ret" | "syscall" };
  explanation: Explanation;   // 来自元数据 + diff(为什么变化)
  sourceLine?: SourceLoc;     // 有调试信息时映射到 C++ 源码行(Phase 2)
}
```

## 5. HTTP API 概览(utoipa 生成,前端类型自动同步)

| 方法/路径 | 作用 |
|---|---|
| `GET /api/health` `GET /api/tools` | 健康检查;外部工具可用性探测结果 |
| `GET /api/isas` `GET /api/isas/{id}` | ISA 元数据(寄存器/标志/ABI/注释) |
| `POST /api/experiments/{id}/sessions` | 创建实验会话(实验含初始代码) |
| `POST /api/sessions/{id}/assemble` | 汇编当前源码(返回错误行号映射) |
| `POST /api/sessions/{id}/load` | 装载机器码到模拟器 |
| `POST /api/sessions/{id}/step` `…/run` `…/pause` `…/reset` | 执行控制 |
| `PATCH /api/sessions/{id}/registers` `PATCH /api/sessions/{id}/memory` | 宿主侧修改状态 |
| `PUT/DELETE /api/sessions/{id}/breakpoints` | 断点管理 |
| `GET /api/sessions/{id}/trace` `GET /api/sessions/{id}/memory` | 轨迹;内存视图(含类型化重解释) |
| `GET/POST /api/courses…` `GET /api/exercises…` | 课程与习题(静态数据服务) |
| `POST /api/exercises/{id}/submit` | 判题(确定性部分走 lab-judge) |
| `GET/PUT /api/progress…` | 学习进度(SQLite) |
| `POST /api/ai/explain` `POST /api/ai/hint` | AI 助教(未配置时返回明确 501) |

Phase 1 只实现模拟器相关子集;模式 B 的调试端点在 Phase 2 以同构形式补入。

## 6. 存储

### 6.1 课程/实验/习题:数据文件(离线可用,git 可版本化)

```
data/courses/stage-02-registers/
├── stage.toml                    # 阶段元数据 + 依赖
└── lesson-01-registers/          # 一节课
    ├── lesson.toml               # 15 步教学模板结构化字段(见 COURSE_DESIGN.md)
    ├── content.md                # 正文(markdown,含嵌入实验锚点)
    ├── experiments/swap.asm.toml # 实验定义:初始代码/断点/期望观察/校验
    └── checks/expected-01.json   # 参考执行结果 golden(由测试生成并复核)
```

### 6.2 SQLite(用户态数据,Phase 1 起)

```sql
CREATE TABLE lesson_progress (lesson_id TEXT PRIMARY KEY, status TEXT, last_experiment TEXT, updated_at INTEGER);
CREATE TABLE exercise_attempt (id INTEGER PRIMARY KEY, exercise_id TEXT, attempt_no INTEGER,
                               status TEXT, hints_shown INTEGER, answer_json TEXT, judged_json TEXT, created_at INTEGER);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);   -- AI 配置等
CREATE TABLE study_log (id INTEGER PRIMARY KEY, kind TEXT, ref_id TEXT, message TEXT, created_at INTEGER);
```

单用户单库(`assemblylab.db`),rusqlite + `spawn_blocking`;迁移用内嵌 SQL 版本号。

## 7. 前端结构(Vue 3)

```
web/src/
├── api/            # openapi-typescript 生成 + 薄封装
├── stores/         # session / course / progress / settings (Pinia)
├── views/
│   ├── HomeView          # 学习首页:进度、推荐下一步、最近实验
│   ├── CourseView        # 三栏:目录 / 教学内容 / 知识点侧栏
│   ├── WorkbenchView     # 实验工作台(核心)
│   ├── CoroutineLabView  # Phase 4 协程专题
│   ├── ExerciseView      # 习题中心 + 判题反馈
│   ├── GraphView         # Phase 7 知识图谱
│   └── SettingsView      # 工具链状态、AI 配置
└── components/workbench/
    ├── ControlBar        # 运行/暂停/单步/步入/步出/重置/断点
    ├── CodePane          # Monaco:源码或汇编,断点点击,当前行高亮
    ├── AsmPane           # 汇编/反汇编,↔源码行互跳(有调试信息时)
    ├── RegisterPanel     # 分组寄存器 + 子寄存器展开 + 点击说明
    ├── FlagsPanel        # 标志位,变化时闪烁高亮
    ├── MemoryViewer      # hex/ASCII/类型化视图,点击地址展开上下文
    ├── StackViewer       # 栈帧可视化(RSP/RBP 标注,返回地址/参数/局部变量)
    ├── TracePanel        # 指令执行轨迹(可回看任意步)
    └── ExplainCard       # 当前指令:before/after + 解释(StepReport 渲染)
```

布局用 splitpanes,面板可折叠/调宽;布局状态存 localStorage。当前执行位置在汇编、反汇编、源码三处统一高亮。

## 8. AI 助教适配(lab-ai)

- OpenAI-compatible:`base_url + api_key + model` 由用户在设置页配置,存 SQLite(settings 表),**不随项目提交**。
- 上下文注入:AI 请求必须携带结构化上下文(当前 StepReport、相关 ISA 元数据、实验目标、用户答案),system prompt 明确「以下状态来自真实引擎,不得改写或臆造」。README 第九节的约束在此落地。
- 降级:未配置或请求失败时,解释卡显示元数据自带的静态解释(无 AI 完整可用);习题的 AI 评价通道仅作为「参考意见」展示,不参与确定性判题。
- 解释层次参数(初学者/C++ 程序员/CPU 原理/编译器实现)作为 UI 上的固定档位,映射到 prompt 模板。

## 9. 决策记录

| # | 决策 | 备选 | 理由 |
|---|---|---|---|
| D-AR-1 | 会话状态在后端 actor 线程持有,前端只发命令 | 无状态 API(每次传完整状态) | 模拟器/调试器天然有状态;actor 避免 Unicorn 句柄跨线程问题;前端无需理解内部布局 |
| D-AR-2 | StepReport 在后端组装(diff+解释) | 前端拿原始事件自行计算 | 解释需要 ISA 元数据(后端权威);避免双端逻辑漂移;轨迹可持久化复用 |
| D-AR-3 | 课程与进度分离:课程=文件,进度=SQLite | 全进 SQLite | 课程要 git 版本化与离线分发;进度是运行期用户态数据 |
| D-AR-4 | AI 适配层可整体禁用,所有功能有非 AI 降级路径 | AI 深度耦合判题/教学 | README 第九节硬性要求 |
| D-AR-5 | 前后端通过生成的 OpenAPI 类型对齐 | 手写 TS 类型 | 消除漂移;类型即文档 |

## 10. 待确认项

1. [ ] workspace 拆分粒度(10 crate 是否过细;可先 5 个:api/core/engine/adapters/storage,Phase 2 再拆)。
2. [ ] 是否需要 WebSocket 推送(run 模式下的流式轨迹);Phase 1 用轮询 REST 足够,run 结束后一次性返回轨迹。
3. [ ] 课程文件中「实验锚点」的最终语法(content.md 里如何嵌入可交互实验)。
