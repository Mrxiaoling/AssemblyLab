# TEST_PLAN — 测试与质量保证

> Phase 0 设计文档 · 状态:待确认
> 上游需求:README 第十二节。核心立场:页面能打开不算完成;教学内容必须与真实执行结果一致。

## 1. 总览(测试金字塔)

```
        E2E 冒烟(手动清单,Playwright 可选)
      ────────────────────────────────────
     内容一致性测试(课程实验可跑通/golden 复核/判题正确)
    ──────────────────────────────────────────
   适配器测试(汇编器/反汇编器/调试器/ISA 数据包完整性)
  ────────────────────────────────────────────
 引擎语义测试(指令语义 golden/标志位/栈/分支/异常)
──────────────────────────────────────────────────
 单元测试(diff 计算、judge、DWARF 解析、DTO 序列化)
```

运行入口:`cargo test --workspace`(后端全部)+ `npm run test`(vitest)+ `cargo test -p lab-judge --test content_tests course_golden_values_match_real_execution`(课程正文数值一致性,见 §5)。

## 2. 引擎语义测试(模式 A 正确性)

### 2.1 指令 golden 测试

- 数据驱动:每个测试用例 = `{源汇编(nasm), 断言}`;运行管线真实执行(真实 nasm → Unicorn → 逐步)。
- 断言三层:最终寄存器值、每步 reg/flags diff(与 golden JSON 完全一致)、内存写入序列。
- golden 由执行生成后**人工复核入库**;变更 golden 必须附带原因,防止「让测试通过」式改数据。
- 覆盖起点(Phase 1,约 120 用例):
  - MOV 族(立即数/寄存器/内存、MOVSX/MOVZX)、LEA 计算式
  - ADD/SUB/INC/DEC/NEG、CMP、TEST
  - MUL/IMUL/DIV/IDIV(含溢出与除零错误路径)
  - AND/OR/XOR/NOT(清 CF/OF 语义)
  - SHL/SHR/SAR(含移位计数取模 8 的坑)
  - PUSH/POP、CALL/RET(栈值与 RSP 变化精确断言)
  - JMP/Jcc 全条件(ZF/SF/OF/CF 组合矩阵)+ LOOP
  - SYSCALL 桩(write/exit)
- 异常路径:未映射访问、栈溢出、除零 → 断言错误信息与停止位置(不是崩溃)。

### 2.2 标志位专项

- 参数化矩阵:对 `ADD/SUB/CMP/AND/…` × 边界操作数(`0`、`1`、`-1`、`0x7FFF…F`、`0x8000…0`、相等/借位/进位组合)断言 CF/ZF/SF/OF/PF 真值表。
- 有符号 vs 无符号:同一条 `SUB` 在两个视角下的解读用例(教学文案引用的数据来源)。

### 2.3 子寄存器写语义专项

- `mov eax, …` 后断言 RAX 高 32 位清零;`mov al, …` 后断言高 56 位保留;`movzx/movsx` 混合用例。ARM64 阶段加 W 寄存器零扩展对照。

### 2.4 桩syscall

- `write(1, buf, n)` → 输出出现在会话日志;`exit(n)` → 会话终止码;未知 syscall 号 → 明确错误。

## 3. ABI 一致性测试

- 元数据自检:abi.toml 中 SysV/Win64 的寄存器角色与文献核对(用例内嵌权威表格快照)。
- 交叉验证(Phase 2):同一 C++ 函数 `clang -S -O0` 分别按两个 ABI 目标编译,提取参数寄存器使用,断言与 abi.toml 推导一致——保证「ABI 页面展示的规则」与真实编译器行为对齐。
- 栈对齐用例:call 前后 RSP 的 16 字节对齐断言(SysV:call 前 RSP%16==0;Win64:预留影子空间后同值)。

## 4. 适配器测试

| 适配器 | 用例 |
|---|---|
| NasmAdapter | 正常汇编;语法错误→错误行号/列号/消息的映射正确;`.bss`/`.data`/标签解析;长文件性能 |
| IcedAdapter | 与机器码 golden 对比反汇编文本;操作数/flags 读写元数据抽查 |
| DWARF/gimli(Phase 2) | 手工构造带 -g 的程序:行号表解析、源码↔指令映射往返一致 |
| GdbMiAdapter(Phase 2) | 断点/单步/寄存器/内存/调用栈/源映射,全部对 `gdb --batch` 基线输出断言 |
| IsaRegistry | 每个 ISA 数据包 schema 校验;寄存器别名无环;flags 位号唯一;instruction-notes 与 iced 的 flags 元数据一致性(冲突即测试失败) |

## 5. 教学内容一致性测试(README 十二节「教学内容测试」逐条落地)

1. **实验能否运行**:`content_tests::mode_a_experiments_actually_run` 遍历所有课程,提取实验定义 → 真实汇编+装载+逐步执行,任何失败即测试失败。
2. **参考结果正确**:每实验 golden(`expected-*.json`)与引擎实际执行全量比对;「参考结论文案中的数值」用模板引用 golden,杜绝手抄数字。
3. **习题判题准确**:每题自带 `judge_cases`(正确答案、典型错误答案、边界);判题器逐例断言(含提示梯度行为:第一次错误→提示1,第二次→提示2,主动看答案→解析)。
4. **内容与真实执行一致**:正文 content.md 中出现的寄存器值/标志结论若用 `{{exp:swap:step:4:reg:RAX}}` 模板语法,则渲染时取 golden,构建时校验模板可解析(禁止手写数值进入正文的关键位置)。
5. **课程依赖合理**:stage/lesson 的 `requires` 字段构成 DAG,校验无环、无缺失引用、推荐顺序可拓扑排序。

## 6. 前端测试(vitest)

- 纯逻辑:diff 渲染映射、轨迹状态机、断点集合操作、类型化内存视图解码(UTF-8/LE 数字)。
- 组件冒烟:面板随 mock StepReport 正确更新(不追求像素级测试)。

## 7. 端到端冒烟(每阶段验收时手动执行,清单化)

1. 打开第一课 → 点「运行实验」→ 工作台出现汇编与初始状态。
2. 单步 5 次:寄存器/标志/内存变化与解释卡一致。
3. 在 `add` 行设断点 → 运行 → 停在断点 → 删除断点。
4. 改一个寄存器值继续运行 → 观察结果符合预期。
5. 改代码重新汇编 → 新轨迹正确。
6. 完成一道「预测结果」习题 → 判题与提示链路正确。
7. 断网状态重复 1–6(验证离线);AI 未配置时解释卡为静态解释。

## 8. 性能基线(本地,失败即排查)

| 指标 | 目标 |
|---|---|
| 单步往返(含渲染) | < 50 ms |
| 1 万步 run 轨迹返回 | < 2 s |
| 汇编(<200 行) | < 300 ms |
| 工作台首屏 | < 1 s(本地资源) |

## 9. 决策记录

| # | 决策 | 备选 | 理由 |
|---|---|---|---|
| D-TST-1 | 引擎正确性用 golden JSON 全量快照,人工复核后入库 | 只断言最终值 | 中间步进错误(如标志位错)只看终值发现不了;golden 让任何行为变化可见 |
| D-TST-2 | 内容一致性作为 `content_tests` 集成测试(非独立二进制) | 只靠人工核对 | README 十一节第 6/7 条的机械化落实;早期设计的 `lab-content-check` 独立 crate 未创建 |
| D-TST-3 | 正文关键数值用模板引用 golden | 手写数字 + 复查 | 教学文案与执行真值强绑定,改实验自动暴露过期文案 |
| D-TST-4 | E2E 用手动清单起步,Playwright 延后 | 直接上 Playwright | 本地单用户工具,先保验收效率,前端稳定后再自动化 |

---

## 附:测试环境依赖说明(P7 收官补充)

- **WSL 依赖**:compiler_tests / debug_tests(真实 g++/gdb)依赖 WSL2 Ubuntu-22.04 + g++/gdb/socat。已加运行时探测(`wsl_available()`):WSL 不可用时**跳过并输出 skip 日志**(CI/其他机器不再大面积失败);wsl_probe 默认 `#[ignore]`。
- **纯本地测试**(无 WSL 依赖):unicorn / nasm / iced / isa / judge / engine / content_tests 在任意 Linux/Windows 机器可跑。
- **Golden/正文数值**:已建立 `[[lesson.experiment.golden]]`(step/reg/value)+ `content_tests::course_golden_values_match_real_execution`,用真实模拟引擎逐步取寄存器快照比对,改实验源码却忘改正文会失败。**当前仅 stage01-lesson01 声明了 3 条 golden**,其余 26 门课正文数值仍为手写,逐课补齐属后续工作。习题 `answer_md` 已 20/20 覆盖,并有断言防止回落。
- **前端**:vue-tsc 类型检查 + vite build;vitest 覆盖 memdecode 解码逻辑(8 用例)与 `utils/diff.ts` 的 LCS/行数上限(6 用例)。
