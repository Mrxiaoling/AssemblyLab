// 汇编逐行对照:归一化文本 + LCS 对齐。抽成纯函数便于单测,也避免大输入阻塞 UI。
// O(n·m) 空间,因此必须限制行数(调用方与 UI 都做门控)。

export interface DiffRow {
  no: number
  a: string | null
  b: string | null
  same: boolean
}

export const DIFF_LINE_LIMIT = 1500

export function normalizeAsm(s: string): string {
  return s.replace(/\s+/g, ' ').trim()
}

/** 返回 null 表示输入过大,不应计算。 */
export function lcsDiff(aTexts: string[], bTexts: string[]): DiffRow[] | null {
  if (aTexts.length > DIFF_LINE_LIMIT || bTexts.length > DIFF_LINE_LIMIT) return null
  const n = aTexts.length
  const m = bTexts.length
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0))
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] = aTexts[i] === bTexts[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
    }
  }
  const rows: DiffRow[] = []
  let i = 0
  let j = 0
  const push = (a: string | null, b: string | null, same: boolean) => {
    rows.push({ no: rows.length + 1, a, b, same })
  }
  while (i < n && j < m) {
    if (aTexts[i] === bTexts[j]) {
      push(aTexts[i], bTexts[j], true)
      i++
      j++
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      push(aTexts[i], null, false)
      i++
    } else {
      push(null, bTexts[j], false)
      j++
    }
  }
  while (i < n) { push(aTexts[i], null, false); i++ }
  while (j < m) { push(null, bTexts[j], false); j++ }
  return rows
}
