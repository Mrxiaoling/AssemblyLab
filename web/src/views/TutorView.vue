<script setup lang="ts">
// AI 助教(P7 第二批)—— 本地检索式引擎:全部回答基于真实数据(ISA 注释/课程/习题解析/图谱)
import { ref } from 'vue'
import { NButton, NInput, NTag, NAlert, NSpin } from 'naive-ui'
import { api } from '../api/client'

const question = ref('')
const busy = ref(false)
const result = ref<any | null>(null)
const error = ref('')

async function ask() {
  if (!question.value.trim() || busy.value) return
  busy.value = true
  error.value = ''
  try {
    result.value = await (await fetch('/api/tutor', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ question: question.value.trim() }),
    })).json()
  } catch (e) {
    error.value = String((e as Error).message)
  } finally {
    busy.value = false
  }
}

function goExercise(id: string) {
  window.location.hash = `/exercises?id=${id}`
}
function goLesson(id: string) {
  window.location.hash = `/learn?lesson=${id}`
}
function goKnowledge(id: string) {
  window.location.hash = `/knowledge`
  void id
}
function exportAll() {
  window.open('/api/export/all')
}
</script>

<script lang="ts">
export default { name: 'TutorView' }
</script>

<template>
  <div style="max-width: 900px; margin: 0 auto; padding: 20px">
    <h2 style="color:#63e2b7; margin-top: 0">🧑‍🏫 助教</h2>
    <p style="color:#888; font-size:13px">
      本地检索式引擎:回答全部基于真实数据(ISA 注释 / 27 门课程 / 20 道习题解析 / 知识图谱),
      <b>零编造、零网络</b>。README 第九节的 AI 职责(解释指令/分析行为/对比 C++/盲点分析/推荐)由确定性检索覆盖。
    </p>
    <n-input
      v-model:value="question"
      type="textarea"
      placeholder="如:lock cmpxchg 怎么实现自旋锁? / 协程帧里保存了什么? / 端序是什么?"
      :rows="2"
    />
    <div style="margin: 10px 0; display: flex; gap: 10px; align-items: center">
      <n-button type="primary" size="small" :disabled="busy" @click="ask">
        {{ busy ? '检索中…' : '🔍 提问' }}
      </n-button>
      <span style="color:#666; font-size:12px">
        试试:lock cmpxchg · 协程帧 · 端序 · 虚表 · stlxr · as-if 规则
      </span>
    </div>

    <n-alert v-if="error" type="error" size="small">{{ error }}</n-alert>

    <n-spin :show="busy">
      <template v-if="result">
        <div class="block">
          <div class="b-title">📗 30 秒版</div>
          <p style="margin: 4px 0">{{ result.layers.quick }}</p>
          <div style="display:flex; gap:6px; flex-wrap:wrap">
            <n-tag v-for="m in result.matched_nodes" :key="m" size="small" type="info" :bordered="false">{{ m }}</n-tag>
          </div>
        </div>

        <div class="block">
          <div class="b-title">📖 深入版(ISA 注释 + 相关课程)</div>
          <div v-for="(d, i) in result.layers.deep" :key="i" class="deep-line">{{ d }}</div>
        </div>

        <div class="block">
          <div class="b-title">🧪 动手版(以真实执行验证)</div>
          <div v-for="(h, i) in result.layers.hands_on" :key="i" class="deep-line">· {{ h }}</div>
        </div>

        <div class="block" v-if="result.blindspots.length">
          <div class="b-title" style="color:#e0a05f">🔎 盲点分析(来自你的真实作答记录)</div>
          <div v-for="(b, i) in result.blindspots" :key="i" style="color:#e0a05f; font-size:13px">· {{ b }}</div>
        </div>

        <div class="block" v-if="result.recommended.length">
          <div class="b-title" style="color:#7ab0e0">🎯 推荐下一步</div>
          <n-button v-for="(r, i) in result.recommended" :key="i" size="tiny" block secondary type="info"
                    style="margin-top:4px; justify-content: flex-start" @click="goExercise(r[0])">
            {{ r[1] }}
          </n-button>
        </div>

        <div class="block" v-if="result.related_nodes.length">
          <div class="b-title">🔗 关联知识点</div>
          <n-tag v-for="r in result.related_nodes" :key="r" size="small" :bordered="false"
                 style="cursor:pointer; margin: 2px 4px 2px 0" @click="goKnowledge(r)">{{ r }}</n-tag>
        </div>

        <div class="block" v-if="result.related_lessons.length">
          <n-button v-for="(l, i) in result.related_lessons" :key="i" size="tiny" block secondary
                    style="margin-top:4px; justify-content: flex-start" @click="goLesson(l[0])">
            课程:{{ l[1] }}
          </n-button>
        </div>
      </template>
    </n-spin>

    <hr style="border-color:#2a2a2a; margin: 20px 0" />
    <h3 style="color:#63e2b7">📦 离线课程包导出</h3>
    <p style="color:#888; font-size:13px">
      自包含单文件 HTML(全部课程正文 + 习题解析内嵌,file:// 直接可开;动手实验回平台用真实引擎)。
    </p>
    <n-space>
      <n-button size="small" @click="exportAll">
        ⬇ 导出全部课程(单文件)
      </n-button>
    </n-space>
  </div>
</template>

<style scoped>
.block { border: 1px solid #2a2a2a; border-radius: 6px; padding: 10px 14px; margin: 10px 0; background: #181818; }
.b-title { color: #63e2b7; font-size: 13px; margin-bottom: 6px; }
.deep-line { font-size: 13px; color: #cfcfcf; padding: 2px 0; }
</style>
