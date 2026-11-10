// 后端 API 客户端(fetch 封装,统一错误形态)
import type {
  HealthResp,
  MemoryResp,
  DisasmResp,
  RunResp,
  SessionResp,
  StepResp,
  TraceResp,
  ApiErrorBody,
} from './types'

export class ApiError extends Error {
  kind: string
  line?: number
  constructor(kind: string, message: string, line?: number) {
    super(message)
    this.kind = kind
    this.line = line
  }
}

const API_TOKEN = (import.meta as ImportMeta & { env?: Record<string, string | undefined> }).env
  ?.VITE_API_TOKEN;

async function request<T>(url: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers);
  if (!headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
  if (API_TOKEN) headers.set('X-AssemblyLab-Token', API_TOKEN);
  const resp = await fetch(url, { ...init, headers });
  if (!resp.ok) {
    let body: ApiErrorBody | null = null
    try {
      body = (await resp.json()) as ApiErrorBody
    } catch {
      /* 非 JSON 错误体 */
    }
    const e = body?.error
    throw new ApiError(e?.kind ?? `http_${resp.status}`, e?.message ?? resp.statusText, e?.line)
  }
  return (await resp.json()) as T
}

export interface LessonListItem {
  id: string
  title: string
  stage: number
  requires: string[]
  minutes: number
  progress: string
}

export interface ExerciseListItem {
  id: string
  title: string
  kind: string
  stage: number
  lesson: string
  difficulty: number
}

export interface CourseSource {
  isa: string
  source: string
  watch: string[]
  /** 实验入口:asm=模式A工作台 | debug=真机调试 | compare=优化对比 */
  mode: string
  /** 建议编译器(协程课 = clang++) */
  compiler: string
}

export interface TutorResp {
  layers: { quick: string; deep: string[]; hands_on: string[] }
  matched_nodes: string[]
  blindspots: string[]
  recommended: [string, string][]
  related_nodes: string[]
  related_lessons: [string, string][]
}

export interface LlmStatusResp {
  configured: boolean
  model: string | null
  secure: boolean
  hint: string
}

export interface LlmResp {
  model: string
  answer: string
  context_notes: number
}

export interface JudgeItem {
  key: string
  prompt: string
  actual: string
  ok: boolean
}

export interface SubmitResp {
  passed: boolean
  items: JudgeItem[]
  attempt_no: number
  error: string | null
  hint: string | null
}

export interface DebugSessionResp {
  id: number
  asm_lines: { addr: number | null; text: string; function: string | null; source_line: number | null }[]
  compiler: string
  opt: string
  state: { terminated: boolean; stop_reason: string | null; step_count: number }
  breakpoints: number[]
}

export interface DebugStepResp {
  reports: any[]
  state: { terminated: boolean; stop_reason: string | null; step_count: number }
}

export const api = {
  debugCreate: (source: string, opt: string, compiler = 'g++', isa = 'x86-64') =>
    request<DebugSessionResp>('/api/debug/sessions', {
      method: 'POST',
      body: JSON.stringify({ source, opt, compiler, isa }),
    }),

  debugRun: (id: number) =>
    request<DebugStepResp>(`/api/debug/sessions/${id}/run`, { method: 'POST' }),

  debugStep: (id: number, into: boolean, count = 1) =>
    request<DebugStepResp>(`/api/debug/sessions/${id}/step`, {
      method: 'POST',
      body: JSON.stringify({ into, count }),
    }),

  debugFinish: (id: number) =>
    request<DebugStepResp>(`/api/debug/sessions/${id}/finish`, { method: 'POST' }),

  debugBreakpoints: (id: number, lines: number[]) =>
    request<number[]>(`/api/debug/sessions/${id}/breakpoints`, {
      method: 'PUT',
      body: JSON.stringify({ lines }),
    }),

  debugFrames: (id: number) =>
    request<{ frames: any[]; variables: [string, string][] }>(`/api/debug/sessions/${id}/frames`),

  debugWatchpoints: (id: number, expressions: string[]) =>
    request<string[]>(`/api/debug/sessions/${id}/watchpoints`, {
      method: 'PUT',
      body: JSON.stringify({ expressions }),
    }),

  debugMemory: (id: number, addr: string, len = 128) =>
    request<{ addr: string; hex: string; ascii: string }>(
      `/api/debug/sessions/${id}/memory?addr=${encodeURIComponent(addr)}&len=${len}`,
    ),

  debugCoro: (id: number) =>
    request<{
      compiler: string
      timeline: {
        line: number
        func: string | null
        reason: string
        coro_index: number | null
        rip: string | null
        frame_addr: string | null
        frame_fields: [string, string][]
      }[]
      timeline_error: string | null
    }>(`/api/debug/sessions/${id}/coro`),

  tutor: (question: string) =>
    request<TutorResp>('/api/tutor', {
      method: 'POST',
      body: JSON.stringify({ question }),
    }),

  tutorLlmStatus: () => request<LlmStatusResp>('/api/tutor/llm/status'),

  tutorLlm: (question: string) =>
    request<LlmResp>('/api/tutor/llm', {
      method: 'POST',
      body: JSON.stringify({ question }),
    }),

  compareAsm: (source: string, opts: string[], compiler = 'g++') =>
    request<import('./types').CompareEntry[]>('/api/compare', {
      method: 'POST',
      body: JSON.stringify({ source, opts, compiler }),
    }),

  courses: () => request<LessonListItem[]>('/api/courses'),

  course: (id: string) => request<Record<string, unknown>>(`/api/courses/${id}`),

  courseExperiment: (id: string) => request<CourseSource>(`/api/courses/${id}/experiment`),

  exercises: () => request<ExerciseListItem[]>('/api/exercises'),

  exerciseDetail: (id: string) => request<Record<string, unknown>>(`/api/exercises/${id}`),

  submitExercise: (id: string, answers: Record<string, string>) =>
    request<SubmitResp>(`/api/exercises/${id}/submit`, {
      method: 'POST',
      body: JSON.stringify({ answers }),
    }),

  exerciseAnswer: (id: string) =>
    request<{ answer_md: string | null }>(`/api/exercises/${id}/answer`),

  setProgress: (id: string, status: string) =>
    request<{ lesson_id: string; status: string }>(`/api/progress/${id}`, {
      method: 'PUT',
      body: JSON.stringify({ status }),
    }),

  health: () => request<HealthResp>('/api/health'),

  knowledge: () =>
    request<{
      nodes: { id: string; title: string; stage: number; desc?: string; related: string[] }[]
      edges: [string, string][]
      mastery: Record<string, { total: number; passed: number; attempts: number }>
    }>('/api/knowledge'),

  mistakes: () =>
    request<
      { exercise_id: string; title: string; kind: string; stage: number; status: string; attempt_no: number; knowledge: string[] }[]
    >('/api/mistakes'),

  getIsa: (id: string) =>
    request<{
      id: string
      name: string
      registers: { canonical: string; group: string; subregisters?: { name: string }[] }[]
      flags: { name: string; bit: number; semantics: string }[]
      abis: { id: string; name: string }[]
    }>(`/api/isas/${id}`),

  createSession: (source: string, isa = 'x86-64', abi = 'sysv-amd64') =>
    request<SessionResp>('/api/sessions', {
      method: 'POST',
      body: JSON.stringify({ isa, abi, source }),
    }),

  postSource: (id: number, source: string, isa = 'x86-64') =>
    request<SessionResp>(`/api/sessions/${id}/source`, {
      method: 'POST',
      body: JSON.stringify({ isa, source }),
    }),

  getSession: (id: number) => request<SessionResp>(`/api/sessions/${id}`),

  step: (id: number, count = 1) =>
    request<StepResp>(`/api/sessions/${id}/step`, {
      method: 'POST',
      body: JSON.stringify({ count }),
    }),

  run: (id: number) => request<RunResp>(`/api/sessions/${id}/run`, { method: 'POST' }),

  reset: (id: number) => request<SessionResp>(`/api/sessions/${id}/reset`, { method: 'POST' }),

  patchRegisters: (id: number, values: Record<string, string>) =>
    request<SessionResp>(`/api/sessions/${id}/registers`, {
      method: 'PATCH',
      body: JSON.stringify({ values }),
    }),

  setBreakpoints: (id: number, addresses: string[]) =>
    request<string[]>(`/api/sessions/${id}/breakpoints`, {
      method: 'PUT',
      body: JSON.stringify({ addresses }),
    }),

  memory: (id: number, addr: string, len = 256) =>
    request<MemoryResp>(`/api/sessions/${id}/memory?addr=${encodeURIComponent(addr)}&len=${len}`),

  patchMemory: (id: number, addr: string, hexStr: string) =>
    request<{ written: number }>(`/api/sessions/${id}/memory`, {
      method: 'PATCH',
      body: JSON.stringify({ addr, hex: hexStr }),
    }),

  trace: (id: number) => request<TraceResp>(`/api/sessions/${id}/trace`),

  disasm: (id: number) => request<DisasmResp>(`/api/sessions/${id}/disasm`),
}
