// 会话状态(Pinia):工作台全部状态的唯一来源
import { defineStore } from 'pinia'
import { api, ApiError } from '../api/client'
import type {
  AssembleInfo,
  DisasmResp,
  Hex,
  SessionResp,
  SessionState,
  StepReport,
  TraceEntry,
  RunSummary,
} from '../api/types'

export const DEFAULT_SOURCE = `[bits 64]
org 0x400000
global _start

_start:
    mov eax, 10
    mov ebx, 20
    add rax, rbx
    mov [result], rax
    xor ecx, ecx
    sub eax, 5
    mov rax, 60
    mov edi, 7
    syscall

result: dq 0
`

export const DEFAULT_SOURCE_ARM64 = `// ARM64(GNU 语法):ret 跳 X30(已预置终止哨兵)
.global _start
_start:
    mov     x0, #10
    mov     x1, #20
    add     x2, x0, x1
    adds    x3, x2, #5          // 35,置 NZCV
    str     x3, [sp, #-16]!
    ldr     x4, [sp], #16
    mov     x0, x4
    ret

value:  .quad 0
`

interface IsaDetailInfo {
  id: string
  name: string
  registers: { canonical: string; group: string; subregisters?: { name: string }[] }[]
  flags: { name: string; bit: number; semantics: string }[]
}

interface SessionStoreState {
  sessionId: number | null
  isa: string
  isaDetail: IsaDetailInfo | null
  source: string
  image: AssembleInfo | null
  snapshot: [string, Hex][]
  state: SessionState | null
  breakpoints: Hex[]
  lastReports: StepReport[]
  lastRun: RunSummary | null
  disasm: DisasmResp | null
  trace: TraceEntry[]
  traceDropped: number
  busy: boolean
  error: string | null
}

export const useSessionStore = defineStore('session', {
  state: (): SessionStoreState => ({
    sessionId: null,
    isa: 'x86-64',
    isaDetail: null,
    source: DEFAULT_SOURCE,
    image: null,
    snapshot: [],
    state: null,
    breakpoints: [],
    lastReports: [],
    lastRun: null,
    disasm: null,
    trace: [],
    traceDropped: 0,
    busy: false,
    error: null,
  }),

  getters: {
    pcRegister(state): string {
      return state.isaDetail?.registers.find((r) => r.canonical === 'PC') ? 'PC' : 'RIP'
    },
    flagsRegister(state): string {
      return state.isa === 'arm64' ? 'NZCV' : 'RFLAGS'
    },
    rip(state): Hex | null {
      const found = state.snapshot.find(([n]) => n === this.pcRegister)
      return found ? found[1] : null
    },
    regMap(state): Map<string, Hex> {
      return new Map(state.snapshot)
    },
    breakpointsSet(state): Set<Hex> {
      return new Set(state.breakpoints)
    },
  },

  actions: {
    applySession(resp: SessionResp) {
      this.sessionId = resp.id
      this.image = resp.image
      this.snapshot = resp.snapshot
      this.state = resp.state
      this.breakpoints = resp.breakpoints
    },

    async create(isa?: string) {
      this.busy = true
      this.error = null
      try {
        if (isa && isa !== this.isa) {
          // 切换 ISA:保留当前源码(课程实验预载),开全新会话
          this.isa = isa
          this.sessionId = null
          this.isaDetail = null
        }
        const abi = this.isa === 'arm64' ? 'aapcs64' : 'sysv-amd64'
        const resp = await api.createSession(this.source, this.isa, abi)
        this.applySession(resp)
        this.isaDetail = await api.getIsa(this.isa)
        this.lastReports = []
        this.lastRun = null
        this.trace = []
        await this.loadDisasm()
        await this.loadTrace()
      } catch (e) {
        this.error = errText(e)
      } finally {
        this.busy = false
      }
    },

    /** 用户主动切换架构:换默认源码 + 全新会话(与课程预载不同) */
    async switchIsa(isa: string) {
      if (isa === this.isa) return
      this.isa = isa
      this.source = defaultSourceFor(isa)
      await this.create()
    },

    async reassemble() {
      if (!this.sessionId) return
      this.busy = true
      this.error = null
      try {
        const resp = await api.postSource(this.sessionId, this.source, this.isa)
        this.applySession(resp)
        this.lastReports = []
        this.lastRun = null
        await this.loadDisasm()
        await this.loadTrace()
      } catch (e) {
        this.error = errText(e)
        // 汇编错误带行号 → 供编辑器定位
        if (e instanceof ApiError && e.line) this.error = `第 ${e.line} 行: ${e.message}`
      } finally {
        this.busy = false
      }
    },

    async step(count = 1) {
      if (!this.sessionId || this.busy) return
      this.busy = true
      this.error = null
      try {
        const resp = await api.step(this.sessionId, count)
        this.lastReports = resp.reports
        this.lastRun = null
        this.state = resp.state
        await this.refresh()
      } catch (e) {
        this.error = errText(e)
      } finally {
        this.busy = false
      }
    },

    async run() {
      if (!this.sessionId || this.busy) return
      this.busy = true
      this.error = null
      try {
        const resp = await api.run(this.sessionId)
        this.lastRun = resp.outcome
        this.lastReports = []
        this.state = resp.state
        await this.refresh()
        await this.loadTrace()
      } catch (e) {
        this.error = errText(e)
      } finally {
        this.busy = false
      }
    },

    async reset() {
      if (!this.sessionId || this.busy) return
      this.busy = true
      this.error = null
      try {
        const resp = await api.reset(this.sessionId)
        this.applySession(resp)
        this.lastReports = []
        this.lastRun = null
        this.trace = []
        await this.loadDisasm()
      } catch (e) {
        this.error = errText(e)
      } finally {
        this.busy = false
      }
    },

    async patchRegister(name: string, value: string) {
      if (!this.sessionId) return
      this.busy = true
      this.error = null
      try {
        const resp = await api.patchRegisters(this.sessionId, { [name]: value })
        this.applySession(resp)
      } catch (e) {
        this.error = errText(e)
      } finally {
        this.busy = false
      }
    },

    async toggleBreakpoint(addr: Hex) {
      if (!this.sessionId) return
      const next = new Set(this.breakpoints)
      if (next.has(addr)) next.delete(addr)
      else next.add(addr)
      this.busy = true
      this.error = null
      try {
        this.breakpoints = await api.setBreakpoints(this.sessionId, [...next])
      } catch (e) {
        this.error = errText(e)
      } finally {
        this.busy = false
      }
    },

    async refresh() {
      if (!this.sessionId) return
      try {
        const resp = await api.getSession(this.sessionId)
        this.applySession(resp)
      } catch (e) {
        this.error = errText(e)
      }
    },

    async loadDisasm() {
      if (!this.sessionId) return
      try {
        this.disasm = await api.disasm(this.sessionId)
      } catch (e) {
        this.error = errText(e)
      }
    },

    async loadTrace() {
      if (!this.sessionId) return
      try {
        const t = await api.trace(this.sessionId)
        this.trace = t.entries
        this.traceDropped = t.dropped
      } catch (e) {
        this.error = errText(e)
      }
    },
  },
})

/** 各架构的默认示例源码 */
export function defaultSourceFor(isa: string): string {
  return isa === 'arm64' ? DEFAULT_SOURCE_ARM64 : DEFAULT_SOURCE
}

function errText(e: unknown): string {
  if (e instanceof ApiError) return e.message
  if (e instanceof Error) return e.message
  return String(e)
}
