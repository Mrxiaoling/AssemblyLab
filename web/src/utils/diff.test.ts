import { describe, expect, it } from 'vitest'
import { DIFF_LINE_LIMIT, lcsDiff, normalizeAsm } from './diff'

describe('normalizeAsm', () => {
  it('折叠空白并去首尾', () => {
    expect(normalizeAsm('  mov   rax,\trbx  ')).toBe('mov rax, rbx')
  })
})

describe('lcsDiff', () => {
  it('完全相同的两列全部标记为 same', () => {
    const a = ['mov rax, 1', 'add rax, 2', 'ret']
    const rows = lcsDiff(a, [...a])
    expect(rows).not.toBeNull()
    expect(rows!.every((r) => r.same)).toBe(true)
    expect(rows!.map((r) => r.no)).toEqual([1, 2, 3])
  })

  it('一侧插入指令时保持最长公共子序列对齐', () => {
    const a = ['mov rax, 1', 'add rax, 2', 'ret']
    const b = ['mov rax, 1', 'nop', 'add rax, 2', 'ret']
    const rows = lcsDiff(a, b)!
    expect(rows.filter((r) => r.same).map((r) => r.a)).toEqual(['mov rax, 1', 'add rax, 2', 'ret'])
    expect(rows.filter((r) => !r.same).map((r) => r.b)).toEqual(['nop'])
  })

  it('一侧删除指令时标记为仅左侧', () => {
    const rows = lcsDiff(['push rbp', 'mov rbp, rsp', 'ret'], ['push rbp', 'ret'])!
    expect(rows.find((r) => r.a === 'mov rbp, rsp')).toMatchObject({ b: null, same: false })
  })

  it('空输入返回空行列表', () => {
    expect(lcsDiff([], [])).toEqual([])
    expect(lcsDiff(['ret'], [])).toMatchObject([{ a: 'ret', b: null, same: false }])
  })

  it('超过行数上限时拒绝计算', () => {
    const big = Array.from({ length: DIFF_LINE_LIMIT + 1 }, (_, i) => `nop ${i}`)
    expect(lcsDiff(big, ['ret'])).toBeNull()
    expect(lcsDiff(['ret'], big)).toBeNull()
  })

  it('恰好在上限内仍可计算', () => {
    const rows = lcsDiff(Array.from({ length: 3 }, (_, i) => `nop ${i}`), ['nop 1'])
    expect(rows).not.toBeNull()
    expect(rows!.some((r) => r.same)).toBe(true)
  })
})
