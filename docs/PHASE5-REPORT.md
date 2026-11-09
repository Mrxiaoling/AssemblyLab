# Phase 5 阶段报告 — ARM64 实验室

日期:2026-11-09 · 状态:待用户验收(与 Phase 4 一并交付)

## 1. 完成内容

| 任务 | 交付 |
|---|---|
| T5.0 工具链 | WSL 安装 binutils-aarch64-linux-gnu(as/objcopy/objdump)+ gcc/g++-aarch64-linux-gnu(11.4);反汇编用 **yaxpeax-arm 0.5**(纯 Rust,无 C 依赖) |
| T5.1 ARM64 ISA 数据包 | `data/isas/arm64/isa.toml`:X0-X30/W 子寄存器/SP/PC/NZCV/V0-V1(NEON)、AAPCS64 ABI(8 参数寄存器、callee-saved、16 对齐)、14 条指令教学笔记(含 ARM 借位标志与 x86 CF 相反等陷阱)、GNU as 语法 |
| T5.2 常量代码生成 | `tools/unicorn-consts/` 补齐全部架构常量源(含 riscv);生成器产出 **arm64 298 寄存器常量 + arm64_reg_by_name**(W/Q 别名合并)——延续"禁止手抄"管线 |
| T5.3 Unicorn 泛化 | `UcArch::{X86,Arm64}`:uc_open 常量、寄存器表、PC id 按架构分派;x86 保留 INSN syscall 桩,ARM64 走 **UC_HOOK_INTR(SVC)**:write(64)/exit(93) 教学桩,输出进输出面板 |
| T5.4 汇编适配 | `gnu_as.rs`:aarch64-linux-gnu-as(-g 行号)→ objcopy -O binary 扁平二进制 → nm 符号 → objdump -l 行号映射;错误行号扣除 .file 前导;单 .text 教学约束;ulimit+timeout |
| T5.5 反汇编 | `arm64dis.rs`(yaxpeax)与 iced 同形 DisasmLine;`disasm_for/disasm_one_for` 按 ISA 分派;引擎/api 零形态改动 |
| T5.6 引擎泛化 | ISA 包新增 `pc_register/stack_register/flags_register/flags_init/link_register`(默认值保持 x86 兼容);Session 按 `isa.assembler` 分派(nasm / gnu-as-aarch64);build_load_spec ARM64 = X30 预置哨兵(ret 即退出);classify_jump 加 bl/blr/svc;call/ret 解释文案按架构 |
| T5.7 前端 ISA 驱动 | RegisterPanel/FlagsPanel 按 ISA 数据包分组渲染(x86 回退不变);ControlBar **ISA 切换器**(x86-64/ARM64,切换换默认示例源码);课程实验 `?isa=` 预载;compare 编译器加 clang++/aarch64(交叉 objdump 修复) |
| T5.8 阶段十课程 3 门 | AArch64 基础(X/W 移位操作数/adds/stp-ldp/**NEON 128 位读写**)、bl/ret 与 X30 递归、x86-64 vs ARM64 四维度对比方法论;experiment mode=asm + isa=arm64 直达 ARM64 工作台 |
| T5.9 习题 | 2 道 ARM64 predict 题(exercise.isa="arm64" 判题:按 ISA 汇编/执行/比对);judge ABI 按 ISA 取 |
| T5.10 RISC-V 评估 | `docs/RISCV-ASSESSMENT.md`:可行、成本 ≈ ARM64 的 60%、建议 Phase 7 后按需启动(P5 泛化使"ISA 即数据"成立) |

## 2. 关键决策与发现

| # | 决策/发现 | 记录 |
|---|---|---|
| D-5-1 | 反汇编选 **yaxpeax-arm**(纯 Rust)而非 capstone(需 cmake/C 工具链,MSVC 构建成本高);分支目标以相对偏移显示,已注明 | arm64dis.rs |
| D-5-2 | **ARM64 退出机制**:引擎把 X30(LR)预置为哨兵 → 叶函数 `ret` 即退出;**非叶程序用 `svc #0`(exit 93)**——递归课后半段由此设计(main 消费掉 bl 返回地址后 ret 会跳回自身) | lesson02 正文显式讲授该陷阱 |
| D-5-3 | **存量 bug 修复(两架构同)**:单步走到退出哨兵时引擎返回**空报告**且不记 terminal(一直被 run 路径掩盖)——哨兵分支现在产出带 ProgramExit 的 StepEvents | unicorn.rs;content_tests 步进到终态的强校验由此可行 |
| D-5-4 | ARM64 面板只放 X0-X3/X8-X10/X19-X21/X29/X30/SP/PC/NZCV 15 个(教学够用);**习题问值必须用面板级寄存器** | 习题试错修正(X4→X2) |
| D-5-5 | aarch64 对比用**交叉 objdump**(WSL 原生 objdump 不识 aarch64 ELF);compile 的 objdump/notation 按 compiler 前缀分派 | compiler.rs |
| D-5-6 | ARM64 课程源码踩过 `bl` 返回地址语义坑(ret 跳回自身死循环)——已转为教学内容(LIFO 习题第二问即出自此处真实执行) | 失败→课程的直接转化 |
| D-5-7 | compare 实测:x86 O2 `square` = `imul eax,edi` 3 条 vs **ARM64 O2 = `mul w0,w0,w0; ret` 2 条** | 优化对比页可直接复现 |

## 3. 测试结果

- 后端 **19 套件 33 项测试全绿(rc=0)**:含 content_tests 6 项(课程 19 门 schema + 模式 A 实验按 ISA **步进到终态**校验 + ARM64 习题判题自检)
- curl:ARM64 会话创建 → 单步(mov/add lsl#/adds/str 前索引/ldr 后索引)全部正确;ret→哨兵退出;compare aarch64(square= mul w0,w0,w0)
- 浏览器:工作台 ISA 切到 ARM64 → GNU as 示例程序运行到「程序退出」、反汇编(yaxpeax,带机器码字节)、寄存器面板 X/NZCV 分组;协程页与优化对比页回归正常
- 前端 vue-tsc 零错误 + vite build 通过

## 4. README 验收对照

| 标准 | 状态 |
|---|---|
| PRD §9 #12 架构切换 | ✅ 工作台 ISA 选择器 + 课程实验 isa 字段 + 判题按 ISA |
| ROADMAP §5 NEON 基础实验 | ✅ stage10-lesson01 hands-on(movi v0.2d/add v1.2d/str q1 → 内存面板 128 位字节) |
| 第二阶段重点(LDR/STR/ADD/SUB/CMP/B/BL/RET/条件执行/寻址) | ✅ ISA notes 14 条 + 3 门课 |
| C++ 与 ARM 汇编联动 | ✅ compare 的 aarch64-linux-gnu-g++(-O0..O3 + 源码行映射) |

## 5. 已知问题(P6+)

- [ ] ARM64 反汇编分支目标为相对偏移(yaxpeax 无 pc 上下文;可后处理换算绝对地址)
- [ ] NEON 面板仅 V0/V1 两个入口,128 位值经内存视图观察(V 寄存器直读留待 P6 SIMD 专题)
- [ ] compare 的 aarch64 未做 NEON/SVE 高亮
- [ ] DebugView(模式 B)仍固定 x86-64 ISA(ARM64 Linux 真机调试需 WSL aarch64 运行时,P6 评估)

## 6. 待你确认

1. **Phase 4+5 验收动线**:
   - 协程:学习中心 → 7·协程帧 → 打开实验(自动 clang++)→ 协程页签「录制」→ 播放 + 点站点看帧字段
   - ARM64:学习中心 → 10·AArch64 基础 → 打开实验(自动 ARM64 工作台)→ 单步 + hands-on 加 NEON 三行;
   - 对比:优化对比页把编译器切到 aarch64-linux-gnu-g++。
2. Phase 6 范围:OS 与并发专题(syscall 深入/上下文切换原型/SIMD 对比/原子与屏障)——按 ROADMAP §6 执行?确认后进入 Phase 6。
