<script setup lang="ts">
// 知识图谱 / 掌握度 / 错题本(P7 第一批)
// 布局:按阶段分列的 SVG 节点图(边 = 课程/习题 knowledge 标签共现推导);
// 点击节点 → 侧栏显示掌握度 + 关联习题;顶部标签切换图谱 | 错题本。
import { computed, onMounted, ref, watch } from 'vue'
import { NTag, NButton, NTabPane, NTabs, NEmpty } from 'naive-ui'
import { api } from '../api/client'

const graph = ref<{ nodes: any[]; edges: [string, string][]; mastery: Record<string, any> } | null>(null)
const mistakes = ref<any[]>([])
const tab = ref('graph')
const selected = ref<string | null>(null)
const error = ref('')

async function load() {
  try {
    graph.value = await api.knowledge()
    mistakes.value = await api.mistakes()
  } catch (e) {
    error.value = String((e as Error).message)
  }
}
onMounted(load)
watch(tab, () => { if (!graph.value) void load() })

const nodeMap = computed(() => new Map((graph.value?.nodes ?? []).map((n) => [n.id, n])))

// 布局:stage 分列(1-10),列内按 title 排序纵向排布
const LAYOUT = computed(() => {
  const nodes = graph.value?.nodes ?? []
  const byStage = new Map<number, any[]>()
  for (const n of nodes) {
    if (!byStage.has(n.stage)) byStage.set(n.stage, [])
    byStage.get(n.stage)!.push(n)
  }
  const stages = [...byStage.keys()].sort((a, b) => a - b)
  const COL = 190
  const ROW = 34
  const maxRows = Math.max(...stages.map((s) => byStage.get(s)!.length), 1)
  const pos = new Map<string, { x: number; y: number }>()
  stages.forEach((s, ci) => {
    const list = byStage.get(s)!.sort((a, b) => a.title.localeCompare(b.title))
    list.forEach((n, ri) => {
      pos.set(n.id, { x: 90 + ci * COL, y: 50 + ri * ROW })
    })
  })
  return { pos, stages, width: 120 + stages.length * COL, height: 80 + maxRows * ROW }
})

function masteryOf(id: string) {
  return graph.value?.mastery?.[id] ?? null
}
function masteryColor(id: string): string {
  const m = masteryOf(id)
  if (!m || !m.total) return '#2e4a42'      // 未做题:暗绿
  const r = m.passed / m.total
  if (r >= 1) return '#63e2b7'              // 全过:亮绿
  if (r > 0) return '#e6c04a'               // 部分:黄
  return '#e05f5f'                          // 做过全错:红
}

// 关联习题(id 含 stage 前缀)
const selectedMistakes = computed(() => {
  if (!selected.value) return []
  return mistakes.value.filter((m) => m.knowledge.includes(selected.value!))
})

function goExercise(id: string) {
  window.location.hash = `/exercises?id=${id}`
}
</script>

<script lang="ts">
export default { name: 'KnowledgeView' }
</script>

<template>
  <div class="kg-root">
    <div class="kg-head">
      <n-tabs v-model:value="tab" type="line" size="small" style="flex: 1">
        <n-tab-pane name="graph" tab="知识图谱与掌握度" />
        <n-tab-pane name="mistakes" tab="错题本" />
      </n-tabs>
    </div>

    <n-alert v-if="error" type="error" size="small">{{ error }}</n-alert>

    <!-- 图谱 -->
    <div v-if="tab === 'graph' && graph" class="kg-main">
      <div class="kg-canvas">
        <svg :viewBox="`0 0 ${LAYOUT.width} ${LAYOUT.height}`" :style="{ width: '100%', minHeight: '60vh' }">
          <!-- 边 -->
          <line
            v-for="(e, i) in graph.edges" :key="i"
            :x1="LAYOUT.pos.get(e[0])?.x" :y1="LAYOUT.pos.get(e[0])?.y"
            :x2="LAYOUT.pos.get(e[1])?.x" :y2="LAYOUT.pos.get(e[1])?.y"
            :stroke="selected && (e[0] === selected || e[1] === selected) ? '#63e2b7' : '#2a3a34'"
            stroke-width="1"
          />
          <!-- 节点 -->
          <g
            v-for="n in graph.nodes" :key="n.id"
            :transform="`translate(${LAYOUT.pos.get(n.id)?.x ?? 0},${LAYOUT.pos.get(n.id)?.y ?? 0})`"
            style="cursor: pointer"
            @click="selected = selected === n.id ? null : n.id"
          >
            <circle
              r="11"
              :fill="masteryColor(n.id)"
              :stroke="selected === n.id ? '#e8f5ee' : '#1a2620'"
              stroke-width="2"
            />
            <text y="26" text-anchor="middle" font-size="10.5" fill="#9ab5a8">{{ n.title }}</text>
            <text v-if="masteryOf(n.id)?.total" y="-16" text-anchor="middle" font-size="9" fill="#5f7a6e">
              {{ masteryOf(n.id).passed }}/{{ masteryOf(n.id).total }}
            </text>
          </g>
        </svg>
        <div class="legend">
          <span><i style="background:#2e4a42" /> 未做题</span>
          <span><i style="background:#e05f5f" /> 做过未过</span>
          <span><i style="background:#e6c04a" /> 部分通过</span>
          <span><i style="background:#63e2b7" /> 全部通过</span>
        </div>
      </div>

      <div class="kg-side">
        <template v-if="selected && nodeMap.get(selected)">
          <h3>{{ nodeMap.get(selected)!.title }}</h3>
          <div style="color:#888; font-size:12px">阶段 {{ nodeMap.get(selected)!.stage }} · {{ selected }}</div>
          <p v-if="nodeMap.get(selected)!.desc" style="color:#999; font-size:13px">{{ nodeMap.get(selected)!.desc }}</p>
          <div class="m-block">
            <div v-if="masteryOf(selected)">
              掌握度:{{ masteryOf(selected).passed }}/{{ masteryOf(selected).total }} 通过 ·
              累计 {{ masteryOf(selected).attempts }} 次作答
            </div>
            <div v-else style="color:#666">还没有关联的作答记录</div>
          </div>
          <div v-if="selectedMistakes.length" class="m-block">
            <b style="color:#e0a05f">错题</b>
            <n-button v-for="m in selectedMistakes" :key="m.exercise_id" size="tiny" block secondary type="warning"
                      style="margin-top:4px; justify-content: flex-start" @click="goExercise(m.exercise_id)">
              {{ m.title }}({{ m.kind }})
            </n-button>
          </div>
          <div class="m-block">
            <b style="color:#7ab0e0">关联节点</b>
            <div style="display:flex; gap:6px; flex-wrap:wrap; margin-top:6px">
              <n-tag v-for="r in nodeMap.get(selected)!.related" :key="r" size="small" :bordered="false"
                     style="cursor:pointer" @click="selected = r">
                {{ nodeMap.get(r)?.title ?? r }}
              </n-tag>
            </div>
          </div>
        </template>
        <n-empty v-else description="点击图上节点查看掌握度与关联" style="margin-top: 20vh" />
      </div>
    </div>

    <!-- 错题本 -->
    <div v-if="tab === 'mistakes'" class="mistakes">
      <n-empty v-if="!mistakes.length" description="没有错题(或还没有作答记录)" style="margin-top: 12vh" />
      <div v-for="m in mistakes" :key="m.exercise_id" class="mistake-row" @click="goExercise(m.exercise_id)">
        <n-tag size="small" type="warning" :bordered="false">{{ ({ wrong: '未通过', revealed: '已看解析' } as Record<string, string>)[m.status] ?? m.status }}</n-tag>
        <span class="mk-title">{{ m.title }}</span>
        <span style="color:#666; font-size:12px">阶段 {{ m.stage }} · {{ m.attempt_no }} 次作答</span>
        <n-tag v-for="k in m.knowledge.slice(0, 3)" :key="k" size="tiny" :bordered="false">{{ k }}</n-tag>
      </div>
    </div>
  </div>
</template>

<style scoped>
.kg-root { height: calc(100vh - 41px); display: flex; flex-direction: column; overflow: hidden; padding: 0 10px; }
.kg-head { flex: none; }
.kg-main { flex: 1; display: flex; gap: 10px; min-height: 0; }
.kg-canvas { flex: 1; overflow: auto; border: 1px solid #2a2a2a; border-radius: 6px; background: #131a16; position: relative; }
.legend { position: sticky; left: 10px; bottom: 8px; display: flex; gap: 14px; font-size: 11px; color: #7a958a; padding: 6px 10px; }
.legend i { display: inline-block; width: 10px; height: 10px; border-radius: 50%; margin-right: 4px; vertical-align: -1px; }
.kg-side { width: 300px; flex: none; border: 1px solid #2a2a2a; border-radius: 6px; padding: 14px; overflow: auto; background: #161616; }
.kg-side h3 { margin: 4px 0 6px; color: #e0e0e0; }
.m-block { margin-top: 14px; font-size: 13px; color: #b5c5bc; }
.mistakes { flex: 1; overflow: auto; padding: 10px 4px; }
.mistake-row { display: flex; gap: 10px; align-items: center; padding: 10px 12px; border: 1px solid #2a2a2a; border-radius: 6px; margin-bottom: 8px; cursor: pointer; background: #181818; }
.mistake-row:hover { border-color: #3a5a52; }
.mk-title { color: #d8d8d8; flex: 1; }
</style>
