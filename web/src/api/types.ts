// 与后端 lab-engine/lab-api DTO 对齐的类型(手写;M2 末期接 openapi-typescript 生成)
// u64 一律为 "0x.." 十六进制字符串

export type Hex = string

export type StopReason =
  | { kind: 'program_exit'; code: number | null }
  | { kind: 'breakpoint'; address: Hex }
  | { kind: 'instruction_limit'; executed: number }
  | { kind: 'timeout' }
  | { kind: 'fault'; message: string }
  | { kind: 'steps_done' }

export interface RegDiffItem {
  name: string
  before: Hex
  after: Hex
  dec: [number, number] | null
}

export interface FlagDiffItem {
  name: string
  before: boolean
  after: boolean
  semantics: string
}

export interface MemChangeItem {
  addr: Hex
  size: number
  value_hex: string
  region: string
}

export interface PcJump {
  from: Hex
  to: Hex
  kind: 'seq' | 'branch' | 'call' | 'ret' | 'syscall'
}

export interface Explanation {
  category: string
  summary: string
  pitfalls: string[]
  detail: string[]
  related_cpp: string | null
  warnings: string[]
}

export interface StepReport {
  step_no: number
  address: Hex
  size: number
  machine_bytes_hex: string
  disasm: string
  mnemonic: string
  after: [string, Hex][]
  reg_diff: RegDiffItem[]
  flag_diff: FlagDiffItem[]
  mem_changes: MemChangeItem[]
  pc_jump: PcJump
  explanation: Explanation
  stop: StopReason | null
}

export interface RunSummary {
  executed: number
  output: string[]
  stop: StopReason
}

export type TraceEntry =
  | { kind: 'step'; step_no: number; report: StepReport }
  | { kind: 'run'; summary: RunSummary }

export interface SessionState {
  terminal: boolean
  stop: StopReason | null
  step_count: number
}

export interface SymbolInfo {
  name: string
  addr: Hex
}

export interface LineMapping {
  line: number
  addr: Hex
  size: number
}

export interface AssembleInfo {
  base: Hex
  entry: Hex
  size: number
  symbols: SymbolInfo[]
  line_map: LineMapping[]
}

export interface SessionResp {
  id: number
  isa: string
  abi: string
  image: AssembleInfo
  snapshot: [string, Hex][]
  state: SessionState
  breakpoints: Hex[]
}

export interface StepResp {
  reports: StepReport[]
  state: SessionState
}

export interface RunResp {
  outcome: RunSummary
  state: SessionState
}

export interface MemoryResp {
  addr: Hex
  region: string
  hex: string
  ascii: string
}

export interface DisasmLineDto {
  address: Hex
  size: number
  text: string
  mnemonic: string
  bytes_hex: string
}

export interface DisasmResp {
  base: Hex
  lines: DisasmLineDto[]
}

export interface TraceResp {
  entries: TraceEntry[]
  dropped: number
}

export interface HealthResp {
  ok: boolean
  tools: {
    nasm: string | null
    unicorn: boolean
    wsl_gpp: string | null
    mingw: string | null
  }
}

// P3:优化级别对比
export interface CompareAsmLine {
  addr: string | null
  text: string
  function: string | null
  source_line: number | null
}

export interface CompareEntry {
  opt: string
  compiler: string
  compiler_version: string
  asm_lines: CompareAsmLine[]
  instruction_count: number
}

export interface ApiErrorBody {
  error: { kind: string; message: string; line?: number }
}
