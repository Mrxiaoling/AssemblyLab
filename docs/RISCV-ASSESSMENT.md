# RISC-V 实验室可行性评估(Phase 5 交付 · ROADMAP §5)

日期:2026-11-09 · 结论:**可行,成本 ≈ ARM64 的 60%**;建议 Phase 7 后按需启动

## 1. 结论速览

| 维度 | 现状 | RISC-V 接入成本 |
|---|---|---|
| 模拟器 | Unicorn 2.1.4 内置 UC_ARCH_RISCV(RV32/RV64) | unicorn.rs 加一个 UcArch 分支(照抄 Arm64 分支,~40 行) |
| 寄存器常量 | `tools/unicorn-consts/riscv_const.py` **已随 wheel 提取**(本次确认) | 生成器加一段,产物 consts.rs 追加 ~200 常量 |
| 汇编器 | WSL `binutils-riscv64-linux-gnu`(apt 可装,riscv64-linux-gnu-as) | gnu_as.rs 参数化 30 行(工具名 + 段检查复用) |
| 反汇编 | yaxpeax 生态有 `yaxpeax-riscv`(纯 Rust);或交叉 objdump | 新模块 ~80 行(复用 arm64dis 形状) |
| ISA 数据包 | data/isas/riscv64/isa.toml:X0-x31/ABI 命名(a0-a7,t0-t6)/PC | 新文件,无 schema 改动 |
| 退出机制 | ecall(Linux ABI:a7=93 exit) | INTR/INSN 桩 + a0 哨兵双保险,同 arm64 模式 |
| 判题/API/前端 | ISA 全参数化已完成(P5) | **零改动**——assemble_for/disasm_for/面板/习题 isa 字段全按 ISA 分派 |
| 教学内容 | 阶段三(第三阶段:RISC-V)课表 | 2-3 门课 + 2 题 |

## 2. 关键技术事实(已验证)

1. `pip download unicorn` 的 2.1.4 wheel 自带 `riscv_const.py`——寄存器常量可代码生成,无手抄风险(与 x86/arm64 同管线)。
2. Unicorn RISC-V 的 syscall 走 `UC_HOOK_INTR`(ecall),`a7` 存编号、`a0-a2` 参数——与 ARM64 的 `X8/X0-X2` 完全同构,桩逻辑可参数化。
3. Ubuntu 22.04 有 `gcc-riscv64-linux-gnu`/`binutils-riscv64-linux-gnu` 候选包(apt 安装,无需额外源);objdump -l 行号映射与 arm64 管线一致。
4. P5 完成的 ISA 泛化(assembler/emulator_arch/pc/stack/flags/link_register/panel 全在 isa.toml)是本评估可信度的根据:ARM64 接入时 **lab-api/lab-judge/前端零架构特判**(除 ISA 包自身),RISC-V 将复制这一体验。

## 3. 与 ARM64 接入的对照(实际工作量)

| 工作项 | ARM64(P5 实际) | RISC-V(预估) |
|---|---|---|
| Unicorn 泛化 | 枚举 + 分支 + INTR 桩 | +1 枚举值,桩改寄存器名(~1h) |
| 常量生成 | 生成器扩展 + wheel 提取 | 已就绪(~15min) |
| 汇编适配 | gnu_as.rs 全新(~150 行) | 参数化复用(~30 行改动) |
| 反汇编 | yaxpeax-arm 新模块(~100 行) | yaxpeax-riscv 同形(~100 行) |
| ISA 包 | 17 面板寄存器 + 14 条 notes | 同量级(~2h) |
| 课程 | 3 门 + 2 题 | 2-3 门 + 2 题(~1 天) |
| 风险 | — | rv64 单步/内存 hook 语义差异需一次小探针(参照 P5 的 SMC 排查法) |

## 4. 建议与不做的理由(现在)

- **建议时机**:Phase 7(学习系统完善)之后,或当用户提出 RISC-V/嵌入式需求时——课程与引擎管线已具备"ISA 即数据"的扩展点,晚做不亏。
- **现在不做的理由**:README 第十三节 P5 交付清单只含"RISC-V 评估报告";十阶段课程表把 RISC-V 列为"扩展架构";平台当前 ISA 数(2)已满足 x86/ARM 对比教学的核心场景,再扩一架构的边际教学收益低于把协程/OS 专题做深。

## 5. 若启动,最小里程碑(供未来参考)

1. M1(半天):常量 + UcArch::Riscv + INTR 桩 + ISA 包 + health 探测 → 工作台能跑 `li a0, 10; ret` 级程序
2. M2(1 天):gnu_as 参数化 + yaxpeax-riscv 模块 + 判题(习题 isa=riscv64)→ 课程 2 门 + 2 题
3. M3(可选):compare 加 riscv64-linux-gnu-g++ → 三架构同源对比页
