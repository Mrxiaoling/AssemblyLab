<script setup lang="ts">
// 源码编辑器(Monaco + x86-nasm 高亮 + 汇编错误行标记)
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import * as monaco from 'monaco-editor'
import { asmLanguageId } from '../../monarch/asm'
import { useSessionStore } from '../../stores/session'

const store = useSessionStore()
const host = ref<HTMLDivElement>()
let editor: monaco.editor.IStandaloneCodeEditor | null = null
let errorDecor: string[] = []
let currentDecor: string[] = []

onMounted(async () => {
  await import('../../monarch/asm')
  editor = monaco.editor.create(host.value!, {
    value: store.source,
    language: asmLanguageId,
    theme: 'vs-dark',
    fontSize: 13,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    automaticLayout: true,
    tabSize: 4,
  })
  editor.onDidChangeModelContent(() => {
    store.source = editor!.getValue()
  })
})

onBeforeUnmount(() => editor?.dispose())

// 汇编错误行标红
watch(
  () => store.error,
  (err) => {
    if (!editor) return
    const model = editor.getModel()!
    const match = /第 (\d+) 行/.exec(err ?? '')
    errorDecor = model.deltaDecorations(errorDecor ?? [], [])
    if (match) {
      const line = Number(match[1])
      errorDecor = model.deltaDecorations([], [
        {
          range: new monaco.Range(line, 1, line, 1),
          options: { isWholeLine: true, className: 'asm-error-line' },
        },
      ])
    }
  },
)

// 点击反汇编行 → 定位到对应源码行(存在调试行号映射时)
watch(
  () => store.focusSourceLine,
  (line) => {
    if (!editor || !line) return
    editor.revealLineInCenter(line)
    editor.setPosition({ lineNumber: line, column: 1 })
    editor.focus()
  },
)

// 当前执行位置:通过 line_map 找 RIP 对应源码行
watch(
  () => store.rip,
  (rip) => {
    if (!editor || !store.image || !rip) return
    const model = editor.getModel()!
    model.deltaDecorations(currentDecor ?? [], [])
    const lm = store.image.line_map.find((m) => m.addr.toLowerCase() === rip.toLowerCase())
    if (!lm) return
    currentDecor = model.deltaDecorations([], [
      {
        range: new monaco.Range(lm.line, 1, lm.line, 1),
        options: { isWholeLine: true, className: 'asm-current-line' },
      },
    ])
  },
)
</script>

<template>
  <div class="panel" style="height: 100%">
    <div class="panel-title">源代码(NASM)</div>
    <div ref="host" class="panel-body" />
  </div>
</template>

<style>
.asm-error-line {
  background: rgba(255, 60, 60, 0.18);
}
.asm-current-line {
  background: rgba(64, 160, 255, 0.18);
  border-left: 2px solid #40a0ff;
}
</style>
