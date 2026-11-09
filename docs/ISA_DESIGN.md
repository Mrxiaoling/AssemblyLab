# ISA_DESIGN — 指令集架构适配层设计

> Phase 0 设计文档 · 状态:待确认
> 上游需求:README 第四节「体系结构支持」、第十节要求 4「ISA 必须采用独立适配器架构」

## 1. 目标与设计原则

平台的执行引擎、教学界面、课程数据都必须**不感知具体 ISA**。x86-64 只是被注册进系统的第一个 ISA,而不是被写死的默认值。

原则:

1. **ISA 是数据 + 适配器,不是散落在代码里的 if-else。** 寄存器表、标志位表、ABI 定义、教学注释都以结构化数据文件形式存在,随 ISA 打包。
2. **执行真值来自模拟器,ISA 元数据只负责"解释"。** 寄存器变化由 Unicorn 等引擎执行产生(见 EXPERIMENT_ENGINE.md),ISA 层提供寄存器命名、别名关系、标志位语义、教学说明,供 UI 和 AI 助教引用。
3. **同一 ISA 可挂多种 ABI。** x86-64 必须同时支持 System V AMD64 与 Windows x64,实验可切换。
4. **新增 ISA 的成本有明确上限:** 一个定义文件包 + 一个模拟器映射 + 一个汇编器/反汇编器适配,不触碰平台其余代码。

## 2. 核心抽象

### 2.1 IsaDefinition(ISA 定义,数据驱动)

```toml
# data/isas/x86-64/isa.toml(示意)
[isa]
id = "x86-64"
name = "Intel/AMD x86-64"
endian = "little"
emulator_arch = "unicorn:x86"      # 模拟器映射,见 EXPERIMENT_ENGINE.md
emulator_mode = "unicorn:x86-64"
assembler = "nasm"                 # 主汇编器
assembler_syntax = "intel"
disassemblers = ["iced-x86"]       # 反汇编后端(纯 Rust)
byte_unit_names = ["byte", "word", "dword", "qword"]

[isa.memory_map]                   # 教学沙箱的固定内存布局(模拟器模式下)
code   = { base = "0x00400000", size = "0x100000", name = "代码段" }
stack  = { base = "0x7ff000000000", size = "0x100000", name = "栈", grows = "down" }
data   = { base = "0x00600000", size = "0x100000", name = "数据段" }
heap   = { base = "0x01000000", size = "0x100000", name = "堆", grows = "up" }
```

### 2.2 寄存器模型

```toml
# data/isas/x86-64/registers.toml(节选)
[[register]]
canonical = "RAX"        # 唯一 id
size = 64
aliases = ["EAX", "AX", "AL", "AH", "RAX"]   # 同一物理寄存器的视图
group = "general"        # general | pointer | status | segment | vector
role_notes = { zh = "累加器;函数返回值默认经 RAX 返回(SysV 与 Win64 一致)" }

[[register.subregister]] # 供 UI 展开子视图与解释写语义
name = "EAX"
offset = 0
size = 32
write_semantics = "zero_extend_upper"   # 关键教学点:写 32 位清零高 32 位

[[register.subregister]]
name = "AL"
offset = 0
size = 8
write_semantics = "preserve_upper"      # 写 8/16 位保留高位

[[register]]
canonical = "RSP"
group = "pointer"
role_notes = { zh = "栈顶指针;PUSH/POP/CALL/RET 隐式读写它" }
special = "stack_pointer"

[[register]]
canonical = "RFLAGS"
group = "status"
special = "flags"
size = 64
```

要点:

- **写语义(write_semantics)是 x86-64 最重要的教学点之一**,必须进元数据:写 32 位寄存器零扩展高位、写 8/16 位保留高位;ARM64 的 W 寄存器统一为零扩展;RISC-V 按指令区分。UI 点击子寄存器时要显示这条规则。
- 寄存器按 `group` 分组渲染:通用、指针、状态、向量(XMM/YMM)、段(仅展示,教学沙箱不模拟段基址语义)。
- 每个寄存器带 `role_notes`(用途说明)+ 可选 `abi_role`(在当前 ABI 下的职责,如 `arg1`、`return`),ABI 部分单独建模(见 2.4)。

### 2.3 标志位模型

```toml
# data/isas/x86-64/flags.toml(节选)
[[flag]]
name = "ZF"
bit = 6
mnemonic = "Zero Flag"
semantics = "结果为 0 时置 1"
watch = true            # 默认在标志面板高亮显示
[[flag]]
name = "CF"
bit = 0
semantics = "无符号进位/借位;无符号比较看它"
watch = true
[[flag]]
name = "OF"
bit = 11
semantics = "有符号溢出;有符号比较看 SF^OF"
watch = true
[[flag]]
name = "SF"
bit = 7
semantics = "结果最高位(有符号负数)"
watch = true
[[flag]]
name = "PF"
bit = 2
semantics = "低 8 位中 1 的个数为偶数"
watch = false
[[flag]]
name = "AF"
bit = 4
semantics = "低 4 位半进位(BCD 用)"
watch = false
[[flag]]
name = "DF"
bit = 10
semantics = "串操作方向"
watch = false
```

- 标志位是 ISA 数据,不是 UI 硬编码。ARM64 是 NZCV 四个标志(C V N Z),RISC-V 基础整数指令集**没有**标志寄存器——ISA 层用 `has_flags = false` 表达,UI 隐藏对应面板。
- 标志面板的"受影响标志"提示来自指令元数据(2.5),**实际变化值来自模拟器执行**(禁止用元数据伪造)。

### 2.4 ABI / 调用约定模型

```toml
# data/isas/x86-64/abi.toml(节选)
[[abi]]
id = "sysv-amd64"
name = "System V AMD64(Linux/macOS)"
[[abi.integer_arg_regs]]
order = 1
reg = "RDI"
[[abi.integer_arg_regs]]
order = 2
reg = "RSI"
# ... RDX, RCX, R8, R9;浮点参数走 XMM0-XMM7
return_reg = "RAX"
caller_saved = ["RAX", "RCX", "RDX", "RSI", "RDI", "R8", "R9", "R10", "R11"]
callee_saved = ["RBX", "RBP", "R12", "R13", "R14", "R15"]
stack_alignment = 16
red_zone = 128                # System V 允许 -128 字节红区,Win64 没有
shadow_space = 0
syscall = { conv = "syscall", nr_reg = "RAX", args = ["RDI", "RSI", "RDX", "R10", "R8", "R9"] }

[[abi]]
id = "win-x64"
name = "Windows x64"
[[abi.integer_arg_regs]]
order = 1
reg = "RCX"
# ... RDX, R8, R9
return_reg = "RAX"
caller_saved = ["RAX", "RCX", "RDX", "R8", "R9", "R10", "R11"]
callee_saved = ["RBX", "RBP", "RSI", "RDI", "R12", "R13", "R14", "R15"]  # 注意 RSI/RDI 属于 callee-saved
stack_alignment = 16
red_zone = 0
shadow_space = 32             # 调用方必须预留 32 字节
syscall = { conv = "none", note = "Windows 不暴露稳定的 syscall 约定,用户态走 Win32 API" }
```

- 同一实验可选 ABI,后端据此生成不同的函数调用样板(prologue 参数摆放),对比实验(第四阶段实验 7)直接利用此切换。
- **模拟器模式下的 syscall 策略:** 只实现 System V 的最小子集(`write`、`exit`,必要时 `read`、`brk`),用 Unicorn 的 INSN hook 拦截 `syscall` 指令并以教学版实现,不做真实系统调用。Win64 样板程序通过宿主提供的自定义退出标记(如调用 `hlt` 或约定的终止函数)结束。详见 EXPERIMENT_ENGINE.md。

### 2.5 指令教学元数据(解释层,非执行层)

```toml
# data/isas/x86-64/instruction-notes.toml(节选)
[[note]]
mnemonic = "MOV"
category = "数据传送"
summary = "在寄存器/内存/立即数之间复制数据,不影响任何标志位"
pitfalls = ["不能内存到内存直接 MOV", "写 32 位寄存器会清零高 32 位"]
related_cpp = "变量赋值、指针解引用读写"

[[note]]
mnemonic = "LEA"
category = "地址计算"
summary = "计算地址但访问内存,常被编译器用来做乘加运算"
pitfalls = ["lea 用 32 位寄存器做目标时同样零扩展高位"]
related_cpp = "&a[i]、取成员地址;编译器优化中的 x*4+8"

[[note]]
mnemonic = "ADD"
category = "算术"
summary = "加法,按结果设置 CF/ZF/SF/OF/PF/AF"
flags_effect = { sets = ["CF", "ZF", "SF", "OF", "PF", "AF"] }
related_cpp = "+、+=、无符号与有符号共用同一指令,区别只在标志位解读"
```

- `flags_effect` 等**只用于教学提示**。UI 上真正展示的标志变化一律取自模拟器执行前后的 RFLAGS diff(规则见 EXPERIMENT_ENGINE.md §4)。元数据与执行结果不一致时,以执行结果为准并记录告警(测试体系会校验两者一致性,见 TEST_PLAN.md)。
- 这套注释数据同时是 AI 助教的资料库(见 ARCHITECTURE.md AI Tutor 一节):AI 解释指令时,引用的是被验证过的元数据 + 当前真实执行状态,而不是自由发挥。

## 3. x86-64 第一阶段交付内容

Phase 1 必须完成的 ISA 数据包:

- 寄存器:RAX/RBX/RCX/RDX/RSI/RDI/RBP/RSP/R8–R15/RIP/RFLAGS,含 EAX/AX/AL 等 24 个常用别名视图。
- 标志位:CF/PF/AF/ZF/SF/OF/DF(IF/TF 只在讲解中断时提及,面板不展示)。
- ABI:sysv-amd64 与 win-x64(参数寄存器、返回值、caller/callee-saved、16 字节对齐、红区/影子空间差异)。
- 指令注释:第二阶段课程涉及的基础子集(约 30 条):MOV/MOVZX/MOVSX、LEA、ADD/ADC、SUB/SBB、INC/DEC、NEG、MUL/IMUL、DIV/IDIV、CMP、AND/OR/XOR/NOT、TEST、SHL/SHR/SAR、PUSH/POP、CALL/RET、JMP/Jcc 系列、LOOP、NOP、SYSCALL(桩)。
- 内存布局:2.1 的教学沙箱四段映射。

明确**不做**(记录为边界):长模式以外的兼容模式、段式内存语义、AVX-512、真实系统调用。

## 4. ARM64/AArch64(Phase 5 目标形态)

先给出设计结论,细化在 Phase 5 前补充:

| 维度 | x86-64 | AArch64 | 元数据如何表达 |
|---|---|---|---|
| 寄存器 | R0–R15 | X0–X30,SP 独立(ZR=X31 视图) | registers.toml,`aliases = ["W0", ...]` 表达 W/X 关系 |
| 子寄存器写语义 | 32 位清零高位 | **W 寄存器一律零扩展**(规则统一,更简单) | write_semantics |
| 条件标志 | EFLAGS 六标志 + DF 等 | NZCV(N Z C V)+ 可选 PSTATE 位 | flags.toml,`watch` 控制 |
| 条件执行 | Jcc 指令族 | CSEL/CSINC/B.cond 条件码后缀 | instruction-notes 的 category |
| 调用 | CALL/RET | BL(X30 存返回地址)/RET | abi.toml + 指令注释 |
| 参数寄存器 | 见 2.4 | X0–X7,返回 X0(AAPCS64) | abi.toml |
| PC | RIP 可作寄存器读取 | PC 不可作普通寄存器寻址 | registers.toml `special = "pc"` |
| 内存模型 | 小端 | 小端(A64) | isa.toml endian |

模拟器:Unicorn 支持 ARM64(UC_ARCH_ARM64 / UC_MODE_ARM);汇编器用 `clang --target=aarch64-linux-gnu -c` 交叉汇编(不依赖本机 ARM 硬件),反汇编用 capstone 或 yaxpeax-arm(Phase 5 前选型确认)。NEON 教学在 Phase 6,Unicorn 对 V 寄存器读写需走 128 位读接口,spike 阶段已确认的运行时 FFI 需扩展(V0–V31 用 16 字节缓冲),这已计入 Phase 5 任务。

## 5. RISC-V(第三阶段扩展,Phase 5+ 评估)

- Unicorn 支持 RV32/RV64;指令集极简(RV64I 47 条基础指令),非常适合做「自己实现一个迷你 ISA 适配器」的进阶教学案例:让学习者对照平台源码看一个 ISA 如何接入。
- 无标志寄存器 → `has_flags = false`;分支自带比较语义(BEQ/BNE/BLT…)→ instruction-notes 强调与 x86 的「CMP+Jcc」两段式差异。
- 无栈指针强制约束(SP=x2 是约定)、无红区概念、ABI 命名(a0–a7/s0–s11/t0–t6)→ abi.toml 直接表达。

## 6. 新增一个 ISA 的标准流程(验收清单)

1. 新建 `data/isas/<arch>/`(isa.toml、registers.toml、flags.toml、abi.toml、instruction-notes.toml)。
2. 在模拟器适配层登记 arch/mode 常量映射(见 EXPERIMENT_ENGINE.md §3)。
3. 接入汇编器后端(NASM/clang 交叉汇编)与反汇编后端。
4. 提供 syscall 桩或退出约定。
5. 提供至少 1 个冒烟实验 + 引擎语义测试用例(接 TEST_PLAN.md §3)。
6. 课程包可选。

## 7. 决策记录

| # | 决策 | 备选 | 理由 |
|---|---|---|---|
| D-ISA-1 | ISA 定义用 TOML 数据文件,运行时加载 | 硬编码 Rust 常量;TypeScript 前端定义 | 教学内容(注释/说明)会频繁修改,不应重编译;数据文件可直接被测试脚本校验;前端通过 API 获取,不重复维护 |
| D-ISA-2 | 寄存器别名用「canonical + 别名视图」建模,而非独立寄存器 | 每个别名独立建模 | 与真实硬件一致,写语义(零扩展/保留)能精确表达 |
| D-ISA-3 | ABI 作为 ISA 下的可切换 profile | 每个平台一个 ISA(x86-64-sysv / x86-64-win) | README 要求「对比 Windows x64 和 System V」在同一实验内切换;ISA 本体与 OS ABI 本就是两层 |
| D-ISA-4 | 指令注释与执行分离,注释仅供解释/教学 | 用注释驱动执行状态(伪模拟) | 违反 README「不能字符串匹配模拟」红线;注释可错、执行不可错,测试保证一致性 |

## 8. 待确认项

1. [ ] TOML vs YAML vs JSON(倾向 TOML,教学者手写友好;YAML 引号/缩进坑多)。
2. [ ] instruction-notes 是否下沉到 SQLite 支持在线编辑(Phase 7 习题中心再议)。
3. [ ] ARM64 汇编器选 clang 交叉汇编 vs GNU as(Phase 5 前确认,均不阻塞 Phase 1)。
