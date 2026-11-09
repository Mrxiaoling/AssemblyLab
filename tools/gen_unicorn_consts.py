#!/usr/bin/env python3
"""从 unicorn 官方 Python 常量文件生成 Rust 常量模块。

用法:
    python tools/gen_unicorn_consts.py

输入: tools/unicorn-consts/{unicorn_const.py,x86_const.py}
输出: crates/lab-adapters/src/consts.rs

规则(见 spikes/unicorn_step_diff/RESULT.md §2):
常量一律代码生成,禁止手抄——不同版本数值差异大。
"""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "tools" / "unicorn-consts"
OUT = ROOT / "crates" / "lab-adapters" / "src" / "consts.rs"

EXPR = re.compile(r"^\s*([A-Z][A-Z0-9_]*)\s*=\s*(.+?)\s*$")


def parse(path: Path) -> dict[str, int]:
    consts: dict[str, int] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.split("#", 1)[0].strip()
        m = EXPR.match(line)
        if not m:
            continue
        name, expr = m.group(1), m.group(2).strip()
        # 仅支持整数 / 1 << n / 简单加减,足够覆盖 unicorn 常量文件
        try:
            val = eval(expr, {"__builtins__": {}}, consts)  # noqa: S307 - 受控输入
        except Exception:
            continue
        consts[name] = val
    return consts


def main() -> None:
    uc = parse(SRC / "unicorn_const.py")
    x86 = parse(SRC / "x86_const.py")

    def group(names: list[str], src: dict[str, int]) -> list[tuple[str, int]]:
        return [(n, src[n]) for n in names if n in src]

    arch = group(["UC_ARCH_X86", "UC_ARCH_ARM64"], uc)
    modes = group(["UC_MODE_16", "UC_MODE_32", "UC_MODE_64", "UC_MODE_ARM", "UC_MODE_THUMB"], uc)
    prots = group(["UC_PROT_NONE", "UC_PROT_READ", "UC_PROT_WRITE", "UC_PROT_EXEC", "UC_PROT_ALL"], uc)
    errs = group(
        [
            "UC_ERR_OK", "UC_ERR_NOMEM", "UC_ERR_ARCH", "UC_ERR_HANDLE", "UC_ERR_MODE",
            "UC_ERR_VERSION", "UC_ERR_READ_UNMAPPED", "UC_ERR_WRITE_UNMAPPED",
            "UC_ERR_FETCH_UNMAPPED", "UC_ERR_HOOK", "UC_ERR_INSN_INVALID", "UC_ERR_MAP",
            "UC_ERR_WRITE_PROT", "UC_ERR_READ_PROT", "UC_ERR_FETCH_PROT", "UC_ERR_ARG",
            "UC_ERR_READ_UNALIGNED", "UC_ERR_WRITE_UNALIGNED", "UC_ERR_FETCH_UNALIGNED",
            "UC_ERR_HOOK_EXIST", "UC_ERR_RESOURCE", "UC_ERR_EXCEPTION", "UC_ERR_OVERFLOW",
        ],
        uc,
    )
    hooks = group(
        [
            "UC_HOOK_INTR", "UC_HOOK_INSN", "UC_HOOK_CODE", "UC_HOOK_BLOCK",
            "UC_HOOK_MEM_READ_UNMAPPED", "UC_HOOK_MEM_WRITE_UNMAPPED", "UC_HOOK_MEM_FETCH_UNMAPPED",
            "UC_HOOK_MEM_READ_PROT", "UC_HOOK_MEM_WRITE_PROT", "UC_HOOK_MEM_FETCH_PROT",
            "UC_HOOK_MEM_READ", "UC_HOOK_MEM_WRITE", "UC_HOOK_MEM_FETCH", "UC_HOOK_MEM_READ_AFTER",
            "UC_HOOK_INSN_INVALID",
        ],
        uc,
    )
    mems = group(
        [
            "UC_MEM_READ", "UC_MEM_WRITE", "UC_MEM_FETCH",
            "UC_MEM_READ_UNMAPPED", "UC_MEM_WRITE_UNMAPPED", "UC_MEM_FETCH_UNMAPPED",
            "UC_MEM_READ_PROT", "UC_MEM_WRITE_PROT", "UC_MEM_FETCH_PROT",
        ],
        uc,
    )
    queries = group(
        ["UC_QUERY_MODE", "UC_QUERY_PAGE_SIZE", "UC_QUERY_ARCH", "UC_QUERY_TIMEOUT"], uc
    )

    regs = sorted((n, v) for n, v in x86.items() if n.startswith("UC_X86_REG_"))
    ins_sysexit = x86.get("UC_X86_INS_SYSCALL")

    # ARM64(P5):同 DLL 内 arm64_const.py 全量寄存器
    arm64 = parse(SRC / "arm64_const.py")
    a64_regs = sorted((n, v) for n, v in arm64.items() if n.startswith("UC_ARM64_REG_"))
    a64_insvc = arm64.get("UC_ARM64_INS_SVC")

    lines: list[str] = []
    lines.append("//! 由 tools/gen_unicorn_consts.py 生成 —— 请勿手改。")
    lines.append("//! 来源: unicorn-2.1.4 官方 Python 绑定常量(tools/unicorn-consts/)。")
    lines.append("//! 再生成: python tools/gen_unicorn_consts.py")
    lines.append("")
    for title, items in [
        ("arch", arch), ("mode", modes), ("prot", prots), ("err", errs),
        ("hook", hooks), ("mem", mems), ("query", queries),
    ]:
        lines.append(f"// ---- {title} ----")
        for n, v in items:
            lines.append(f"pub const {n}: u32 = {v};")
        lines.append("")

    lines.append("// ---- x86 寄存器(全量) ----")
    for n, v in regs:
        lines.append(f"pub const {n}: u32 = {v};")
    lines.append("")

    lines.append("// ---- ARM64 寄存器(全量,P5) ----")
    for n, v in a64_regs:
        lines.append(f"pub const {n}: u32 = {v};")
    lines.append("")

    if ins_sysexit is not None:
        lines.append(f"pub const UC_X86_INS_SYSCALL: u32 = {ins_sysexit};")
        lines.append("")
    if a64_insvc is not None:
        lines.append(f"pub const UC_ARM64_INS_SVC: u32 = {a64_insvc};")
        lines.append("")

    # 寄存器名 → id 查找("RAX"/"EAX"/"AL"... 全部收录)
    lines.append("/// 寄存器名(任意大小写)→ Unicorn 寄存器 id。")
    lines.append("pub fn x86_reg_by_name(name: &str) -> Option<u32> {")
    lines.append("    match name.to_ascii_uppercase().as_str() {")
    for n, v in regs:
        short = n.removeprefix("UC_X86_REG_")
        lines.append(f'        "{short}" => Some({n}),')
    lines.append("        _ => None,")
    lines.append("    }")
    lines.append("}")
    lines.append("")

    lines.append("/// ARM64 寄存器名(X0/W0/V0/NZCV/PC/SP…)→ Unicorn 寄存器 id。")
    lines.append("pub fn arm64_reg_by_name(name: &str) -> Option<u32> {")
    lines.append("    let up = name.to_ascii_uppercase();")
    lines.append("    // W 寄存器 = 32 位视图(id 独立);V/Q 别名合并")
    lines.append("    let mut aliases = std::collections::HashMap::new();")
    lines.append("    for i in 0..32 {")
    lines.append("        aliases.insert(format!(\"Q{i}\"), format!(\"V{i}\"));")
    lines.append("    }")
    lines.append("    let name2 = aliases.get(&up).cloned().unwrap_or(up);")
    lines.append("    match name2.as_str() {")
    for n, v in a64_regs:
        short = n.removeprefix("UC_ARM64_REG_")
        lines.append(f'        "{short}" => Some({n}),')
    lines.append("        _ => None,")
    lines.append("    }")
    lines.append("}")
    lines.append("")

    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"written {OUT} (x86 {len(regs)} regs, arm64 {len(a64_regs)} regs, {len(errs)} errs, {len(hooks)} hooks)")


if __name__ == "__main__":
    main()
