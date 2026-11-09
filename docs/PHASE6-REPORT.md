# Phase 6 阶段报告 — OS 与并发专题

日期:2026-11-09 · 状态:待用户验收

## 1. 完成内容

| 任务 | 交付 |
|---|---|
| T6.1 探针先行 | 三组探针验证模拟器支持度:x86 `movdqa/paddd`+`lock cmpxchg`(CAS 成功路径 ZF=1、内存换新值)✓;ARM64 `ldaxr/stlxr/dmb` 独占写生效(lock_var=0x43)✓;**发现 GNU as `ldr =label` 字面池重定位坑**(objcopy 不应用 → 读出 0),源码级规避:一律 `adr`(±1MB)取地址 |
| T6.2 ISA notes 扩充 | x86 +6 条(paddd/movdqa/lock/cmpxchg/xadd/mfence,52 条累计);arm64 +5 条(ldaxr/stlxr/dmb/adr/cbnz,22 条累计)——含 ARM 借位标志与 x86 CF 相反、伪共享等陷阱 |
| T6.3 阶段八课程 4 门 | ①write 系统调用之旅(RAX 编号/RDI-RDX 参数/桩语义/特权级边界)②**swapctx 用户态上下文切换原型**(14 条指令手写切换,A/B 双任务来回+退出码验证)③**四类切换辨析**(三根轴对照表:现场多少/谁来切/换不换地址空间;async 成本模型)④SVC 之旅(ARM64,与 x86 syscall 全表对照:X8 编号/write=64/exit=93/ELR-SPSR 硬件保存分工) |
| T6.4 阶段九课程 3 门 | ①SIMD 数组加法(paddd lane 心智模型+NEON 对照+自动向量化条件)②CAS 与自旋锁(lock cmpxchg 完整语义/失败路径 eax 带回现值/ABA/伪共享)③独占访问与内存屏障(ARM64 LL/SC 自旋锁+弱模型 dmb+x86 强模型对照表) |
| T6.5 习题 | 3 道:stage08-q01(write 返回值,x86 判题)/ stage09-q01(CAS 失败路径,ZF+eax 带回值)/ stage10-q03(ARM64 LL/SC,含 W 写清零 X 上半考点)——共 **13 道**;判题自检含错误答案反向断言 |
| T6.6 存量 bug 修复 | ①**toml Index 缺键 panic**(get_course_experiment 对无 watch 的课程 500——P3 已知坑的第二次复发,统一改 .get());②**workbench 路由复用不重建**:learn→workbench→learn→workbench 换实验源码时组件复用导致旧会话(改 watch(immediate) 监听 exp/isa 变化) |
| T6.7 模式 B ARM64 真机评估 | 见 §4:可行但投入产出比低,列为 P7 可选 |

课程总数 **26 门**(P1 起 3 → P3 13 → P5 19 → P6 26),习题 **13 道**。

## 2. 关键决策与发现

| # | 决策/发现 | 记录 |
|---|---|---|
| D-6-1 | **ARM64 取地址必须用 `adr`**:`ldr x2, =label` 的字面池条目是绝对地址,as 留 R_AARCH64 重定位,`objcopy -O binary` 不应用重定位 → 运行时读到 0(取指 @0 fault)。三步探针(普通 ldr/ldaxr/完整自旋锁)定位为源码级问题 | 已写进 arm64 ISA note(adr 条目)+ lesson03 pitfalls + 课程源码注释 |
| D-6-2 | `stlxr` 助记符顺序:状态寄存器在前(`stlxr w3, x4, [x2]`,w3=0 成功)——与直觉相反,note 里显式标注 | ISA notes |
| D-6-3 | 上下文切换原型选 **callee-saved 全套 + 换 rsp + ret** 的经典 swapctx:B 的"从未运行"现场手工构造(假栈 6 占位+入口),对称回收;退出码 = B 写的 counter,让"切换真的发生"可判题 | lesson02 |
| D-6-4 | 四类切换辨析课用**三根轴**(现场/执行者/地址空间)而非四段叙述,并给成本排序 call < swapctx < 线程 < 进程——async 的本质 = 第二类,所以便宜;阻塞 IO 一样贵 | lesson03 对照表 |
| D-6-5 | exec 计数差一与面板寄存器两个旧坑在自检中再犯再修(step:N 的快照在第 N 步后;子寄存器 W5/X5 不在面板)——**参考答案必须来自真实执行而非手算**的纪律再次被证明必要 | content_tests 自检输出即证据 |
| D-6-6 | SSE 实验的内存验证路径:XMM 128 位不进 64 位快照,统一用 **movdqa 写回内存 + int32 视图** 观察——与 P5 NEON 同一手法 | lesson01 |

## 3. 测试结果

- 后端 **19 套件 34 项测试全绿(rc=0)**:content_tests 8 项(22 门 schema、模式 A 实验按 ISA 步进到终态、13 题结构、P3/P5/P6 判题自检、补全题参考答案)
- curl:三组探针(SSE/CAS、独占访问、完整自旋锁)内存终值全部符合预期
- 浏览器:syscall 课(运行到退出+输出面板)、自旋锁课(**退出码 1** = CAS 成功路径真实执行)、26 门课列表渲染;路由复用修复后 workbench 实验跳转链路恢复
- 前端 vue-tsc 零错误

## 4. 模式 B(真机调试)ARM64 支持评估

**结论:可行,列为 P7 可选项,本轮不实现。**
- 需要:WSL `qemu-user-static`(跑 aarch64 ELF)+ `gdb-multiarch`(调试 aarch64)+ compiler.rs 的 objdump 目标选择(已有前缀分派,复用即可)+ DebugSession 的 ISA 参数化(现为固定 x86-64)
- 工作量约 2-3 天;核心难点在 qemu-user 与 socat 传输链的稳定性探针(参照 P2 的 wsl 管道排查法)
- 收益场景:双架构真机调试对比(教学价值中等——阶段六~九的模式 A 已覆盖 ARM64 语义观察)
- 依据 README"不伪造"原则:未实现前,ARM64 课程实验全部走模式 A(模拟器),页面如实标注"进程级限制"或"模拟器"字样

## 5. README 八/九阶段验收对照

| 阶段 | 实验 | 状态 |
|---|---|---|
| 八 | Linux write/exit syscall 逐步 | ✅ stage08-lesson01(x86)+ lesson04(ARM64 SVC) |
| 八 | 上下文切换原型汇编 | ✅ stage08-lesson02 swapctx |
| 八 | 四类切换辨析(函数/用户协程/线程/内核) | ✅ stage08-lesson03 对照表+微实验 |
| 八 | 中断入口分析 | ✅ lesson04 入口分工(硬件 ELR/SPSR vs 软件现场保存;标注"分析型内容以 Linux 源码为准") |
| 九 | SIMD 数组加法 | ✅ stage09-lesson01(手写 SSE + NEON 对照 + compare 自动向量化) |
| 九 | CAS 与 LOCK(std::atomic 汇编) | ✅ stage09-lesson02 + cpp-link(compare 页 lock xadd/ldaddal) |
| 九 | 自旋锁实现 | ✅ lesson02(x86 CAS 版)+ lesson03(ARM64 LL/SC 版) |
| 九 | x86 vs ARM 内存屏障对比 | ✅ lesson03(mfence 几乎不用 vs dmb 处处要;强/弱模型对照表) |

## 6. 待你确认(进入 Phase 7 前)

1. **验收动线**:学习中心 →「9·lock cmpxchg」→ 打开实验 → 运行(退出码 1 = CAS 成功)→ 单步看 ZF;再开「8·write 系统调用」看输出面板;"优化对比"页切 aarch64-linux-gnu-g++ 看 `std::atomic` 的 ldaddal。
2. Phase 7 范围(ROADMAP §7):习题中心全题型(8 种)/知识图谱页/掌握度/错题本/AI 助教/离线课程包/综合项目——体量较大,建议拆两批(先习题全题型+知识图谱,后 AI 助教)。按此执行?确认后进入 Phase 7。
