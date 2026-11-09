# EXPERIMENT_ENGINE — 实验引擎设计

> Phase 0 设计文档 · 状态:待确认(引擎选型已由 spike 实证,见 §8)
> 上游需求:README 第六节。红线:不伪造执行过程;不把超时当完整沙箱;优先复用成熟引擎。

## 1. 定位:双模式引擎

| | 模式 A:指令模拟器 | 模式 B:真实程序调试 |
|---|---|---|
| 用途 | 基础汇编教学(阶段 1–3 及全部逐步实验) | C/C++ 与真实汇编(阶段 4+) |
| 引擎 | Unicorn 2(预编译 DLL,运行时加载) | GDB/MI 子进程(MinGW/WSL gdb) |
| 隔离 | 结构性隔离(SECURITY_DESIGN §2) | 进程级限制(SECURITY_DESIGN §3) |
| 状态真值 | 模拟器执行 | 真实 CPU 执行 |
| 交付 | **Phase 1** | Phase 2 |

两模式向上输出**同构的 `StepEvents`**(before/after 快照 + 内存事件),因此 diff/解释/展示层完全复用——"模式 B 不需要重新设计一遍教学 UI"。

## 2. 模式 A 执行管线(核心设计)

### 2.1 逐步循环(spike 已验证的形态)

```
load:   mem_map(教学沙箱四段, 权限) → mem_write(机器码) → reg_write(初始寄存器/RIP)
loop:   before = snapshot(全部寄存器 + RFLAGS)
        uc_emu_start(rip, code_end, timeout, count=1)        // 恰好一条指令
        after  = snapshot(...)
        mem_writes = drain(mem_hook 缓冲)
        → lab-engine 计算 StepReport(见 §3)
run:    同上,但 count=N 且 code hook 检查断点/超时/指令数上限,命中即 uc_emu_stop
```

关键实现事实(来自 `spikes/unicorn_step_diff/RESULT.md`):

- `count=1` 即单步;错误返回时 RIP 停在出错指令,状态仍可读,可生成友好错误。
- 全域内存钩子的作用域参数是 `begin=1, end=0`(begin>end 恒触发)。
- RIP 必须显式初始化;`uc_mem_map` 为 4 参数(含权限);常量表必须从随包头文件生成,禁止手抄。

### 2.2 EmulatorAdapter 运行形态

- Unicorn 句柄非线程安全 → **actor 线程**独占引擎,`lab-engine` 经命令通道交互(ARCHITECTURE §3)。
- 引擎异常只终止会话线程,后端存活(崩溃边界)。
- RunControl(引擎强制,UI 可见):单次 run 指令数上限 100 万、会话总量 1000 万、墙钟 5s、映射内存 8MB(SECURITY_DESIGN §2.4)。

### 2.3 断点与步进语义

| 操作 | 模式 A 实现 |
|---|---|
| 单步 | count=1 |
| 步入(step into) | 同单步(CALL 自然进入) |
| 步过(step over) | 若当前是 CALL:临时断点设在本指令之后地址,run 到达;否则同单步 |
| 步出(step out) | 读 [RSP] 得返回地址,设临时断点 run 至返回 |
| 断点 | UC_HOOK_CODE 中检查地址集合,命中 → uc_emu_stop + 记录原因 |
| 运行到光标 | 临时断点 |
| 重置 | 重新 load(程序与初始状态由会话保存) |

临时断点与用户断点分开管理;run 结束返回「停止原因」(断点/终止/上限/错误),UI 显示明确。

### 2.4 syscall 桩(教学系统调用)

- x86-64:INSN hook 拦截 `syscall`;按 ABI(sysv-amd64)取 RAX=号,RDI/RSI/RDX/R10/R8/R9=参数。
- 实现集:Linux 语义的 `read(0)`(从会话输入缓冲)、`write(1/2)`(写入会话输出日志)、`exit(n)`(终止,返回码可见)、`brk`(教学沙箱堆段内移动,超出报错)。
- 其余 syscall:一律拒绝并暂停,提示「模拟器未实现该系统调用」。
- Win64 无稳定 syscall 约定 → 模式 A 下 Windows 风格程序以 `ret` 返回到约定的终止桩结束(汇编模板由课程提供)。
- **页面标注**:所有 syscall 行为注明「教学桩实现,非真实内核」。

### 2.5 交互式修改(README 执行控制要求)

- 改寄存器/改内存:两次指令之间任意时刻可执行(`uc_reg_write`/`uc_mem_write`);UI 上修改后立即重渲染快照,下一条指令按新状态执行(spike 已验证该流程)。
- 改代码重编译:重新走 汇编→load→重置;会话保留断点(按地址重新映射,失效断点提示)。
- 修改后历史轨迹保留旧数据并标注「重置于第 N 步」。

## 3. StepReport:diff 与解释的分层(防伪造的关键)

```
引擎层(事实):  StepEvents { before, after, mem_writes, exit }
                 │  只含模拟器真实执行结果,不可注入任何"推测值"
                 ▼
解释层(教学):  lab-engine + IsaRegistry:
                 reg_diff   = before/after 逐寄存器对比(含子寄存器视图展开)
                 flag_diff  = RFLAGS 位级对比 → 标志名(来自 flags.toml)
                 mem_diff   = 写事件 + 类型化重解释(按实验声明的数据类型)
                 pc_jump    = RIP 变化分类(seq/branch/call/ret/syscall)
                 explanation= instruction-notes.toml 的静态说明
                              + 本次真实 diff 的数值引用
                              + 元数据声称的 flags_effect 与实际 diff 的一致性校验
                 (不一致 → UI 显示告警 + 记录日志,内容测试拦截,见 TEST_PLAN §2)
```

**规则:解释文本可以基于元数据生成,但所有数值/标志变化必须引用引擎结果;两层不一致宁可显示错误也不静默修正。**

## 4. 模式 B:GDB/MI 适配(Phase 2 设计预案)

- 进程:`gdb --interpreter=mi2`,命令白名单(-break-insert/-exec-step/-exec-next/-exec-continue/-data-…/-stack-…),绝不透传用户文本。
- MI 解析:自写轻量解析器(README 要求复用成熟接口——GDB/MI 本身即成熟接口;crate 生态评估见 TECH_STACK D-TS-5:`gdbmi` 0.0.2 实验期、`serde_gdbmi` 低活跃,自写 ~300 行解析器风险更低,决策待 Phase 2 spike 后最终确认)。
- 状态映射:`-exec-step`(指令级)→ before/after 寄存器快照;内存写事件通过 `can-list`+观察点(`-break-watch`)近似,或仅对已声明观察的内存范围设硬件观察点;栈帧用 `-stack-list-frames` + `-stack-list-variables`。
- 源码映射:`-g` 编译 → gimli 读行号表(主路径)+ MI `-data-disassemble -s 2` 反汇编带源行标注(交叉校验)。
- Windows 原生(MinGW gdb)与 WSL2 gdb 二选一按目标程序;UI 标注运行环境。

## 5. 汇编/反汇编适配

- 汇编:外部真实工具(nasm `-f bin` + `-l` 列表文件提供行号↔地址映射与符号表)。拒绝内置 keystone(与真实工具行为可能不一致)。源码经临时文件传入(SECURITY §4)。
- 反汇编:iced-x86(纯 Rust)用于指令级展示、编码长度、flags 读写元数据;`objdump` 仅作测试对照,不进运行时依赖。
- 错误映射:nasm 的 `file:line: msg` 解析为前端可定位错误(行号 → Monaco 装饰器)。

## 6. 与 ISA 层的边界

- 引擎代码中不出现 "RAX" 等字面量:寄存器 id 来自 ISA 数据包的模拟器映射段(ISA_DESIGN §2),snapshot/diff 全部走 `RegisterId` 抽象。
- 新增 ISA(ARM64)对引擎的改动预期:常量表 + 128 位向量寄存器读写 + syscall 桩表 —— 均在适配器内,核心循环不变。

## 7. 性能预算

| 操作 | 预算 | 依据 |
|---|---|---|
| 单步(快照×2 + diff) | < 5ms | spike 实测单步远低于此 |
| 1 万步 run | < 2s | Unicorn TCG 执行 + 逐步 hook;若不达标改为批量模式(hook 缓存快照,批量返回) |
| 汇编 200 行 | < 300ms | 外部进程启动 ~50ms + nasm |

## 8. 决策记录:指令执行引擎选型(spike 实证后定稿)

| 路线 | 说明 | 优点 | 缺点 | 结论 |
|---|---|---|---|---|
| A. `unicorn-engine` crate(bindgen+cmake 源码编译) | crates.io 2.1.5,GPL-2.0 | API 现成 | 需要 libclang(本机无,需装 LLVM)、cmake 构建慢、同为 GPL-2 | 备选 |
| **B. 官方预编译 DLL + 运行时加载 + 手写薄 FFI(~150 行)** | PyPI wheel 自带 dll+头文件 | **零构建依赖(spike 实证)、常量可从随包头文件代码生成、升级=换 DLL** | FFI 表面积自维护(~12 个函数,小) | **✅ 采纳(Phase 1)** |
| C. 自研 x86-64 教学解释器(iced-x86 解码 + 手写语义) | 纯 Rust,MIT | 许可干净、教学解释可完全内联 | 语义正确性风险大(标志位/边界情形),与"不重复造轮子"冲突,工作量数月 | 长期后备(若出现分发许可冲突) |

spike 证据:`spikes/unicorn_step_diff/RESULT.md`——单步、寄存器/标志 diff、内存钩子、宿主侧修改、错误路径全部 PASS,且标志位与手工推算一致。

**明确声明(README 要求):** 本引擎模拟的是**指令级语义**(ISA 参考手册层面),不是微架构(流水线/缓存/分支预测)。涉及性能的教学内容用真实硬件测量数据旁证,并显式标注。

## 9. 待确认项

1. [ ] 引擎路线 B 的确认(含 GPL-2.0 后端组件的接受度)。
2. [ ] 模式 B 的 MI 解析器自写方案在 Phase 2 开工前用 3 天 spike 验证(MinGW gdb + WSL gdb 各一)。
3. [ ] RunControl 默认上限值(§2.2)是否符合教学节奏(可放设置页)。
