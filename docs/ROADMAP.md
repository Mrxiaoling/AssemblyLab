# ROADMAP — 开发阶段计划

> Phase 0 设计文档 · 状态:待确认
> 上游需求:README 第十三节。原则:阶段式交付,每阶段有明确交付物、验收标准与「待你确认」的决策门;不擅自跨阶段。

## 0. Phase 0(本文档集)— 状态

交付:`docs/` 10 份设计文档 + `spikes/unicorn_step_diff`(Unicorn Windows 可行性验证,结论写入 EXPERIMENT_ENGINE.md §8)。
**出口条件:你对 10 份文档与关键技术决策确认后,进入 Phase 1。**

## 1. Phase 1 — 最小可用实验室(x86-64 + 模拟器)

目标(README 定义):用户能够通过一个简单的汇编程序,真正观察每条指令的执行过程。

### 里程碑

- **M1 引擎骨架(约第 1–2 周):** 后端能汇编→装载→单步→返回 diff。验收:`cargo test` 通过引擎语义测试子集;curl 可走通 step API。
- **M2 工作台(约第 3–4 周):** 浏览器里完整工作台可用。验收:手动冒烟清单 §TEST_PLAN 7 的 1–5 项通过。
- **M3 课程与习题(约第 5–6 周):** 3 节课 + 2 题型上线。验收:冒烟清单 6–7 项通过;content-check 全绿。
- **M4 收尾验收:** PRD §7 MVP 验收清单全过,输出阶段报告。

### 任务拆分

| 任务 | 内容 | 产出物 | 完成标准 |
|---|---|---|---|
| T1.0 脚手架 | cargo workspace(5 crate:api/core/engine/adapters/storage)+ web(Vite+Vue3+TS+Pinia+Naive)+ 仓库规范(README 补充、.gitignore)+ `scripts/dev.ps1`(一键起前后端) | 可 `cargo run` + `npm run dev` | 两端空壳可启动、健康检查互通 |
| T1.1 Unicorn 正式适配层 | 把 spike 的运行时 DLL 加载器产品化(错误处理、版本校验、路径配置);**常量/寄存器表从随包头文件代码生成(禁止手抄,见 spike RESULT.md §2)**;实现 `EmulatorAdapter`(load/run_steps/run/set_register/write_memory/snapshot/reset);actor 线程与命令通道;RunControl(指令数/超时上限);syscall 桩(write/exit/read/brk);code/mem hook | `lab-adapter-unicorn` + 生成脚本 + 单测 | 语义测试 §2.1 首批 40 用例通过;崩溃边界测试(actor panic 不杀后端)通过 |
| T1.2 x86-64 ISA 数据包 | isa/registers/flags/abi/instruction-notes 五个数据文件 + `lab-isa` 加载与 schema 校验 | `data/isas/x86-64/` | IsaRegistry 测试通过(TEST_PLAN §4);寄存器面板数据源就绪 |
| T1.3 NasmAdapter | 外部进程封装(超时/输出截断/临时目录)、错误行号映射、`-f bin` 产出 + 符号表(nasm `-l`/`.map`)供实验地址解析 | `lab-adapter-tools` | 汇编错误往返用例通过;性能基线达标 |
| T1.4 IcedAdapter | 反汇编 + 指令元数据(flags 读写、操作数),服务解释卡与反汇编面板 | 同上 | 与 golden 机器码对照用例通过 |
| T1.5 引擎编排 | Session(创建/汇编/装载/步进/断点/patch/轨迹环形缓冲)、StepReport 组装(diff+解释)、与 ISA 元数据联动 | `lab-engine` | 单步全链路 curl 测试;轨迹回放 API 正确 |
| T1.6 API 层 | utoipa 全路由(ARCHITECTURE §5 的 P1 子集)+ OpenAPI JSON 导出 + CORS(仅 localhost) | `lab-api` | swagger 可用;前端类型生成脚本就绪 |
| T1.7 前端骨架 | 路由/布局/主题(暗色)/api 客户端(生成类型)/基础 store | `web/` | 打开首页无报错;类型零手写漂移 |
| T1.8 工作台九面板 | ControlBar/CodePane(Monaco+asm 语法高亮+断点)/AsmPane/RegisterPanel(子寄存器展开)/FlagsPanel/MemoryViewer(hex+ASCII)/StackViewer(栈内存视图+RSP 标注)/TracePanel/ExplainCard;splitpanes 布局持久化 | 工作台视图 | PRD US-1~3 手动验收;当前执行位置三视图同步高亮 |
| T1.9 课程框架 | lesson.toml/content.md 解析、15 步渲染组件、实验锚点、进度记录(SQLite) | `lab-storage` + 课程视图 | L1 在课程页可走完 15 步并跳实验 |
| T1.10 首批课程 | L1「一个加法程序的一生」、L2「数据移动」、L3「算术与标志」(COURSE_DESIGN §6 详案),含 golden 与动手校验 | `data/courses/…` | 每课实验可跑、golden 复核、动手校验通过 |
| T1.11 习题引擎 | 判题 L-A(run-and-compare/testcase)+ 提示梯度 + 错题/进度 SQLite;题型:预测结果、补全汇编;习题页 UI | `lab-judge` + 习题视图 | 每题 judge_cases 全过;提示链路正确 |
| T1.12 测试体系落地 | golden 生成/复核脚本、content-check、性能基线测量 | 测试与脚本 | TEST_PLAN §2.1 全量用例、§5 全部机制跑通 |
| T1.13 阶段验收 | 冒烟清单执行、README Phase 1 定义对照、阶段报告(完成内容/变更/未解决问题/下阶段计划/待确认决策) | `docs/PHASE1-REPORT.md` | PRD §7 全过;报告输出 |

**决策门(P1 内,按需向你确认):** ① unicorn.dll 的分发/获取方式固定;② API 面板布局默认值;③ L1–L3 内容终稿。

## 2. Phase 2 — C++ 与汇编联动

| 任务 | 内容 | 完成标准 |
|---|---|---|
| T2.1 CompilerAdapter | Windows 侧 clang/MinGW 探测与调用(`-S`/`-g`/`-c`,可选 WSL 路径),优化级别参数,错误映射 | 四个优化级别产物可用 |
| T2.2 源码↔汇编映射 | gimli 解析 DWARF 行号表;AsmPane 行级映射与双向跳转 | Compiler Explorer 级别的点击联动 |
| T2.3 GdbMiAdapter | MI 子进程管理、断点/单步/读寄存器内存/调用栈/变量;命令白名单 | TEST_PLAN §4 用例全过 |
| T2.4 模式 B 会话 | 调试会话复用工作台;源码视图为主,反汇编切换 | 真实 C++ 程序断点单步 |
| T2.5 栈帧可视化 | StackViewer 升级:返回地址/保存寄存器/参数/局部变量/对齐填充标注 | README 第四阶段栈帧要求全项 |
| T2.6 函数调用/ABI 实验 | 第四阶段课程(调用、栈帧、参数、递归、SysV vs Win64 对比、x86 vs ARM 预备) | 对比实验可切换 ABI |
| T2.7 安全 | Job Object 集成 + 临时目录生命周期 + UI 隔离等级标注 | SECURITY §3 落实 |

验收(README Phase 2):C++ 编译→汇编→反汇编→调试→源映射→栈帧→ABI 实验全链路。

## 3. Phase 3 — 内存与高级实验

内存查看器增强(类型化/结构体布局)、指针/结构体/对齐/端序实验、递归与尾调用、优化级别对比(-O0~-O3)、C++ 对象模型实验(this/虚表/lambda)。验收:第三/五/六阶段首批课程上线,「同一 C++ 源码多优化级别对比」工作台可用。

## 4. Phase 4 — C++20 协程实验室

协程编译环境(clang/gcc 标注)、Coroutine Frame DWARF 解析视图、状态机示意生成、挂起/恢复时间线播放、协程专题页(COURSE_DESIGN §7)。验收:README 第七节 10 个观察点各有一个可运行实验;编译器实现细节均带版本标注。

## 5. Phase 5 — ARM64 实验室

ARM64 ISA 数据包、Unicorn ARM64 适配、clang 交叉汇编、AAPCS64 实验、x86-64↔ARM64 对比课程;RISC-V 评估报告。验收:PRD §9 第 12 条(架构切换)达成;NEON 基础实验(128 位寄存器读写)通过。

## 6. Phase 6 — OS 与并发专题

syscall 深入、上下文切换原型实验、中断入口分析、SIMD 数组加法对比、CAS/LOCK/自旋锁、内存屏障对比(x86/ARM)。验收:README 第八/九阶段实验清单落地,四类切换(函数/用户协程/线程/内核)辨析课程完成。

## 7. Phase 7 — 学习系统完善

习题中心全题型(8 种)、知识图谱页、学习进度完善(掌握度)、错题本、AI 助教(多解释层次、盲点分析、推荐)、离线课程包导出、综合项目(README 第五阶段实验 8:实际项目关键函数分析)。验收:README 十五节 14 条全过。

## 8. 风险登记

| 风险 | 等级 | 缓解 |
|---|---|---|
| Unicorn DLL 与 MSVC 链接的兼容性 | 低(spike 已验证运行时加载路线) | 备选:源码编译(需装 LLVM/libclang)或自研解释器(接口已隔离) |
| Unicorn GPL-2.0 分发约束 | 低(本地使用无义务) | 已记录(TECH_STACK §4);有分发需求时切自研解释器 |
| GDB/MI 在 MinGW 环境的兼容坑 | 中 | Phase 2 首个任务做 3 天 spike;WSL2 路径兜底 |
| 协程帧布局跨编译器差异大 | 中 | 布局一律标注来源编译器;不做通用布局承诺(COURSE_DESIGN §7) |
| 教学内容量大导致拖期 | 中 | 课程内容与平台解耦(数据文件),可分批交付;每阶段只承诺课程子集 |
| Node 版本与 Vite 兼容 | 低 | 默认 Vite 6(TECH_STACK §2) |

## 9. 需要你确认的决策汇总(P1 开工前)

1. **文档总体方向**:10 份文档 + spike 结论是否认可(本文档集)。
2. **引擎路线**:Unicorn 预编译 DLL 运行时加载(EXPERIMENT_ENGINE §8 决策)。
3. **工具安装**:Phase 1 需要 `winget install NASM.NASM`(唯一必需新增工具);LLVM/WSL 工具链 Phase 2 再装。
4. **课程首三课详案**(COURSE_DESIGN §6)。
5. **Vite 6 vs Node 升级**(TECH_STACK §7.1)。

## 10. 待确认项

1. [ ] Phase 1 里程碑节奏(按周估计)是否符合你的时间投入。
2. [ ] 是否在 P1 结束时打 tag/发版(本地仓库建议 git init + 阶段 tag)。
