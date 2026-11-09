// memdecode 单元测试(vitest):类型化解码/端序/浮点/符号
// 运行:npx vitest run
import { describe, expect, it } from 'vitest'
import { decodeValues, chunkCells, typeSize } from './memdecode'

// 0x11223344 小端字节序排列:44 33 22 11
const LE_HEX = '44332211'

describe('typeSize', () => {
  it('byte=1 int32/f32=4 i64/f64=8', () => {
    expect(typeSize('byte')).toBe(1)
    expect(typeSize('i32')).toBe(4)
    expect(typeSize('f32')).toBe(4)
    expect(typeSize('f64')).toBe(8)
  })
})

describe('decodeValues 小端', () => {
  it('uint32:0x11223344 → 287454020(小端读出)', () => {
    const cells = decodeValues(LE_HEX, 'u32', true)
    expect(cells).toHaveLength(1)
    expect(cells[0].text).toBe('287454020')
  })

  it('大端切换:0x44332211', () => {
    const cells = decodeValues(LE_HEX, 'u32', false)
    expect(cells[0].text).toBe('1144201745')
  })

  it('int32 有符号:0x11223344 → 287454020(正数不变)', () => {
    const cells = decodeValues(LE_HEX, 'i32', true)
    expect(cells[0].text).toBe('287454020')
  })

  it('i8:第一个字节 0x44 = 68', () => {
    const cells = decodeValues(LE_HEX, 'i8', true)
    expect(cells[0].text).toBe('68')
  })
})

describe('decodeValues 边界', () => {
  it('长度不足一个单元时返回空', () => {
    // 3 字节不够一个 4 字节单元
    expect(decodeValues('112233', 'u32', true)).toHaveLength(0)
  })

  it('空输入返回空', () => {
    expect(decodeValues('', 'byte', true)).toHaveLength(0)
  })
})

describe('chunkCells', () => {
  it('按 perRow 分组', () => {
    const cells = decodeValues('0102030405060708', 'u8', true)
    const rows = chunkCells(cells, 4)
    expect(rows).toHaveLength(2)
    expect(rows[0]).toHaveLength(4)
  })
})
