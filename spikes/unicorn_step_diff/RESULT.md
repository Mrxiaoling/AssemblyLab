# spike 结果:Unicorn 2 在 Windows 上的单步状态捕获 — 可行 ✅

日期:2026-11-08 · 环境:Windows(x64,build 26300)+ Rust 1.96.1(x86_64-pc-windows-msvc)+ VS 18 Community

## 结论

**Unicorn 2.1.4(官方预编译 DLL,运行时加载)+ 薄 FFI(~150 行)可完整支撑平台"模式 A 指令模拟器"的核心循环**,无需 bindgen / cmake / libclang / pkg-config。已实证:

| 验证目标 | 结果 |
|---|---|
| Windows MSVC 下加载官方预编译 unicorn.dll | ✅ `LoadLibraryA` + `GetProcAddress`,零链接期依赖 |
| x86-64 实例创建、内存映射(带权限)、代码装载 | ✅ |
| **单步执行**(`uc_emu_start(count=1)`) | ✅ 6 条指令逐步精确推进 |
| **逐步寄存器/RFLAGS 前后快照与 diff** | ✅ 与手工推算完全一致(见 §4) |
| 内存写事件捕获(mem hook,全域) | ✅ |
| 宿主中途修改寄存器(教学功能前提) | ✅ |
| 未映射访问错误路径(错误码 + 停止位置) | ✅ `UC_ERR_WRITE_UNMAPPED`,RIP 停在出错指令 |

## 1. 获取 DLL 的推荐路径

**PyPI wheel 优先**(实测比 GitHub release 快约 100 倍,且 wheel 内自带头文件与 Python 常量文件,是 API 事实标准):

```
python -m pip download unicorn --only-binary=:all: --platform win_amd64 -d <dir> --no-deps
# unicorn-2.1.4-cp37-abi3-win_amd64.whl (zip)
#   unicorn/lib/unicorn.dll        ← 运行时加载这个
#   unicorn/include/unicorn/*.h    ← API/常量真值来源
#   unicorn/x86_const.py 等        ← 寄存器/标志 ID 数值(与头文件一致)
```

本仓库的副本在 `spikes/unicorn_step_diff/vendor/pypi-extracted/`。GitHub release 的 `windows-msvc64-shared.7z` 是等价备选(本次网络慢未用完)。

## 2. ⚠️ 该版本 API 与常见文档的差异(spike 实测发现)

此 wheel 虽标 2.1.4,API 比 GitHub 2.1.4 release tag 更新,**以随包头文件为准**:

1. **`uc_mem_map` 是 4 参数**:`(uc, address, size, perms)` —— 经典文档是 3 参数。按 3 参数调用会拿到垃圾 perms → `UC_ERR_ARG(15)`(本 spike 踩过的第一个坑)。
2. 寄存器 ID / hook 类型值与旧版完全不同(旧资料全部作废),必须生成自随包 `x86_const.py`/`unicorn_const.py`。本次验证值:
   - `UC_ARCH_X86=4`,`UC_MODE_64=8`,`UC_PROT_ALL=7`,`UC_MEM_WRITE=17`
   - `UC_HOOK_CODE=4`,`UC_HOOK_MEM_WRITE=2048`(旧版是 `1<<13`/`1<<9`)
   - `UC_X86_REG`:RAX=35 RBP=36 RBX=37 RCX=38 RDI=39 RDX=40 RIP=41 RSI=43 RSP=44 RFLAGS=253
3. **hook 作用域**:`uc_hook_add(..., begin, end)` 中 `(0,0)` 只覆盖地址 0;**全域要用 `begin=1, end=0`**(begin > end 恒触发,见头文件 NOTE 2)。
4. 回调类型:trace 类内存钩子返回 `void`(`uc_cb_hookmem_t`);非法访问钩子返回 `bool`(`uc_cb_eventmem_t`,可拦截访问)。
5. `uc_query(uc, UC_QUERY_PAGE_SIZE=2, &out)` 可用于自检对齐要求(本环境 4096)。

**产品化要求(Phase 1 T1.1):** 寄存器/hook/常量表必须从随包头文件以脚本生成进 Rust,不允许手抄;DLL 版本用 `uc_version` + `uc_query` 双重校验,不匹配则拒绝启动并提示。

## 3. 单步循环的正确形态

```
loop {
    before = snapshot(all_regs)          // uc_reg_read × N(含 RFLAGS)
    rip = before.RIP
    uc_emu_start(uc, rip, code_end, timeout=0, count=1)   // 恰好一条指令
    after = snapshot(all_regs)
    mem_writes = drain(mem_hook_buffer)  // UC_HOOK_MEM_WRITE
    diff(before, after) → StepReport
}
```

- 前置:启动时 `uc_reg_write(RIP, code_base)`(RIP 初始为 0,不会自动指到代码)。
- 错误处理:`uc_emu_start` 返回非 OK 时会话状态仍可读(RIP 停在出错指令),转成友好错误即可。

## 4. 标志位正确性抽查(执行结果 vs 手工推算)

| 指令 | 引擎结果 | 手工推算 | 一致 |
|---|---|---|---|
| `mov eax,10` | RAX=0xA | 0xA | ✅ |
| `add rax,rbx`(10+20) | RAX=0x1E,PF 0→1 | 0x1E=11110b,4 个 1 → 偶校验 PF=1 | ✅ |
| `xor ecx,ecx` | RCX=0,ZF 0→1 | 结果 0 → ZF=1 | ✅ |
| `sub eax,5`(0x999-5) | RAX=0x994,PF 1→0、ZF 1→0 | 0x94=3 个 1 → 奇 → PF=0;非零 → ZF=0 | ✅ |
| `mov [0x2000],rax` | mem[0x2000]=0x999 | 同 | ✅ |

(0x999 是主机中途 patch 的演示值——恰好证明"改寄存器后续指令按新值执行",这是交互教学的关键能力。)

## 5. 踩坑记录(对 Phase 1 有直接价值)

1. 3 参数调用 4 参数 `uc_mem_map` → `UC_ERR_ARG`(见 §2.1)。
2. 手工汇编字节序错误:0x2000 误编码为 `00 02 00 00...`(实为 0x200),排查耗时的根因是**先用非法访问钩子拿到真实出错地址**(`addr=0x200`)才定位。此案例本身就是端序教学的绝佳素材。
3. mem hook 范围 `(0,0)` 陷阱(见 §2.3)。
4. RIP 必须显式初始化。

## 6. 许可证与供应链

- Unicorn 引擎(含该 DLL)为 **GPL-2.0**。本地个人使用无义务;若未来分发产品二进制,后端需遵循 GPL-2.0(详见 docs/TECH_STACK.md §4 与后备方案)。
- 固定获取渠道:PyPI wheel(python 包名 `unicorn`),Phase 1 把 wheel 哈希固化入锁文件说明。

## 7. 对文档集的回写

- `EXPERIMENT_ENGINE.md §8` 决策记录:路线 B(预编译 DLL + 薄 FFI)成为首选,路线 A(unicorn-engine crate,bindgen+cmake)与路线 C(自研解释器)留作后备。
- `TECH_STACK.md` D-TS-1 结论由"待 spike 验证"改为"已验证"。
- `ROADMAP.md` T1.1 增加"常量代码生成脚本"要求。

## 8. 复现步骤

```
cd spikes/unicorn_step_diff
# DLL 已就位于 vendor/unicorn-dist/unicorn.dll(来源见 §1)
cargo run
# 期望输出末尾:[PASS] steps=6 (expected 6)
```
