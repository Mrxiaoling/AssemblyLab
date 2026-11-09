// 内存类型化解码:x86-64 为小端存储;端序可切换用于教学对照(阶段三·端序)
export type MemType =
  | 'byte' // 十六进制字节(默认)
  | 'i8'
  | 'u8'
  | 'i16'
  | 'u16'
  | 'i32'
  | 'u32'
  | 'i64'
  | 'u64'
  | 'f32'
  | 'f64'
  | 'ascii'

export const MEM_TYPES: { value: MemType; label: string; size: number }[] = [
  { value: 'byte', label: '字节(hex)', size: 1 },
  { value: 'i8', label: 'int8', size: 1 },
  { value: 'u8', label: 'uint8', size: 1 },
  { value: 'i16', label: 'int16', size: 2 },
  { value: 'u16', label: 'uint16', size: 2 },
  { value: 'i32', label: 'int32', size: 4 },
  { value: 'u32', label: 'uint32', size: 4 },
  { value: 'i64', label: 'int64', size: 8 },
  { value: 'u64', label: 'uint64', size: 8 },
  { value: 'f32', label: 'float32', size: 4 },
  { value: 'f64', label: 'float64', size: 8 },
  { value: 'ascii', label: 'ASCII', size: 1 },
]

export function typeSize(t: MemType): number {
  return MEM_TYPES.find((x) => x.value === t)?.size ?? 1
}

function bytesOf(hex: string, off: number, n: number): number[] {
  const out: number[] = []
  for (let i = 0; i < n; i++) {
    const b = parseInt(hex.slice((off + i) * 2, (off + i) * 2 + 2), 16)
    out.push(Number.isNaN(b) ? 0 : b)
  }
  return out
}

/** 按端序把 bytes 组成大整数:little=true 时 bytes[0] 为最低有效字节(LE 解码:值 = Σ b[i]<<8i) */
function combine(bytes: number[], little: boolean): bigint {
  let v = 0n
  // LE 解码:从最高位字节开始 shift-left → seq 需反转(bytes[0] 最后进 MSB 之外的位置)
  const seq = little ? [...bytes].reverse() : bytes
  for (const b of seq) v = (v << 8n) | BigInt(b)
  return v
}

function signed(v: bigint, bits: number): bigint {
  const half = 1n << BigInt(bits - 1)
  return v >= half ? v - (1n << BigInt(bits)) : v
}

function fmtInt(v: bigint, radix: 'hex' | 'dec' | 'bin'): string {
  if (radix === 'dec') return v.toString(10)
  if (radix === 'bin') return v.toString(2)
  // hex:负数显示为无进制前缀的补码宽十六进制
  if (v < 0n) {
    const bits = BigInt(v < -(1n << 31n) ? 64 : 32)
    return '0x' + ((1n << bits) + v).toString(16).toUpperCase()
  }
  return '0x' + v.toString(16).toUpperCase()
}

export interface DecodedCell {
  /** 距基地址字节偏移 */
  off: number
  text: string
}

/**
 * 把 hex 字符串(地址 base 起)按类型+端序解码为值序列。
 * 剩余不足一个单元的字节忽略。
 */
export function decodeValues(
  hex: string,
  type: MemType,
  little: boolean,
  radix: 'hex' | 'dec' | 'bin' = 'dec',
): DecodedCell[] {
  const n = typeSize(type)
  const total = Math.floor(hex.length / 2)
  const cells: DecodedCell[] = []
  for (let off = 0; off + n <= total; off += n) {
    let text = ''
    if (type === 'byte') {
      text = hex.slice(off * 2, off * 2 + 2).toUpperCase()
    } else if (type === 'ascii') {
      const b = parseInt(hex.slice(off * 2, off * 2 + 2), 16)
      text = b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '·'
    } else if (type === 'f32' || type === 'f64') {
      const bytes = bytesOf(hex, off, n)
      const buf = new ArrayBuffer(n)
      const view = new DataView(buf)
      const setBytes = little ? bytes : [...bytes].reverse()
      setBytes.forEach((b, i) => view.setUint8(i, b))
      text = (n === 4 ? view.getFloat32(0, true) : view.getFloat64(0, true))
        .toPrecision(8)
        .replace(/\.?0+$/, '')
    } else {
      const v = combine(bytesOf(hex, off, n), little)
      if (type === 'i8') text = fmtInt(signed(v, 8), radix)
      else if (type === 'i16') text = fmtInt(signed(v, 16), radix)
      else if (type === 'i32') text = fmtInt(signed(v, 32), radix)
      else if (type === 'i64') text = fmtInt(signed(v, 64), radix)
      else text = fmtInt(v, radix)
    }
    cells.push({ off, text })
  }
  return cells
}

/** 类型化视图:一行放 perRow 个值 */
export function chunkCells(cells: DecodedCell[], perRow: number): DecodedCell[][] {
  const rows: DecodedCell[][] = []
  for (let i = 0; i < cells.length; i += perRow) rows.push(cells.slice(i, i + perRow))
  return rows
}

export function hx(v: number | bigint): string {
  return '0x' + v.toString(16).toUpperCase()
}
