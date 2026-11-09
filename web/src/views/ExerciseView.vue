<script setup lang="ts">
// 习题中心:题目渲染 + 结构化作答 + 确定性判题反馈 + 提示梯度
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import {
  NCard, NTag, NButton, NInput, NSelect, NSpace, NList, NListItem, NThing,
  NAlert, NEmpty, NSpin, NCollapse, NCollapseItem,
} from 'naive-ui'
import { api, type SubmitResp, type ExerciseListItem } from '../api/client'

const route = useRoute()
const list = ref<ExerciseListItem[]>([])
const detail = ref<Record<string, any> | null>(null)
const answers = ref<Record<string, string>>({})
const result = ref<SubmitResp | null>(null)
const busy = ref(false)
const error = ref('')

const exId = computed(() => String(route.query.id ?? ''))

// 路由 ?id= 变化时切换题目(workbench 同款组件复用问题)
let lastId: string | null = null
watch(
  exId,
  async (id) => {
    if (!list.value.length) await loadList()
    else if (id && id !== lastId) await openExercise(id)
  },
  { immediate: true },
)

async function loadList() {
  list.value = await api.exercises()
  const target = exId.value || list.value[0]?.id
  if (target) {
    lastId = target
    await openExercise(target)
  }
}

async function openExercise(id: string) {
  busy.value = true
  result.value = null
  error.value = ''
  try {
    detail.value = (await api.exerciseDetail(id)) as Record<string, any>
    answers.value = {}
  } catch (e) {
    error.value = String((e as Error).message)
  } finally {
    busy.value = false
  }
}

const QUESTIONS = computed(() =>
  (detail.value?.predict?.questions ?? []) as { key: string; prompt: string; expect_kind: string }[],
)

async function submit() {
  if (!detail.value) return
  busy.value = true
  error.value = ''
  try {
    result.value = await api.submitExercise(detail.value.id, answers.value)
  } catch (e) {
    error.value = String((e as Error).message)
  } finally {
    busy.value = false
  }
}

const kindLabel = (k: string) =>
  ({
    predict: '预测结果',
    fill_blank: '补全指令',
    asm2cpp: '汇编转 C++',
    cpp2asm: 'C++ 转汇编分析',
    fix_bug: 'Bug 修复',
    debug_challenge: '调试挑战',
    programming: '综合编程',
    explain: '原理解释',
  }[k] ?? k)

// quiz(静态问答)/ self_check(自评)题型
const QUIZ = computed(() => (detail.value?.quiz ?? null) as { display: string; questions: { key: string; prompt: string }[] } | null)
const SELF = computed(() => (detail.value?.self_check ?? null) as { display: string; items: string[] } | null)
const FILLISH = computed(() => detail.value?.fill_blank ?? null)  // fill_blank / fix_bug / programming 共用

function toggleSelf(i: number) {
  const key = `item${i}`
  answers.value[key] = answers.value[key] === 'true' ? 'false' : 'true'
}

onMounted(loadList)
</script>

<template>
  <div style="display: flex; height: 100vh; overflow: hidden">
    <div class="sidebar">
      <div class="side-title">习题</div>
      <n-list hoverable clickable show-divider>
        <n-list-item v-for="e in list" :key="e.id" @click="openExercise(e.id)">
          <n-thing :title="e.title">
            <template #header-extra>
              <n-tag size="tiny" :bordered="false">{{ kindLabel(e.kind) }}</n-tag>
            </template>
            <span style="color: #666; font-size: 12px">难度 {{ '★'.repeat(e.difficulty) }}</span>
          </n-thing>
        </n-list-item>
      </n-list>
    </div>

    <div class="content">
      <n-spin :show="busy">
        <n-empty v-if="!detail" description="选择一道习题" style="margin-top: 20vh" />
        <template v-else>
          <div class="ex-head">
            <n-tag type="info" size="small">{{ kindLabel(detail.kind) }}</n-tag>
            <n-tag size="small" :bordered="false">难度 {{ '★'.repeat(detail.difficulty) }}</n-tag>
            <h1>{{ detail.title }}</h1>
            <div class="kn">
              <n-tag v-for="k in detail.knowledge" :key="k" size="tiny" :bordered="false">{{ k }}</n-tag>
            </div>
          </div>

          <n-alert v-if="error" type="error" size="small">{{ error }}</n-alert>

          <!-- 题面 -->
          <n-card title="程序" size="small" style="margin: 12px 0" v-if="detail.predict">
            <pre class="asm-view">{{ detail.predict.asm.trim() }}</pre>
          </n-card>

          <!-- 作答 -->
          <n-card title="你的预测" size="small" style="margin: 12px 0" v-if="QUESTIONS.length">
            <n-space vertical size="medium">
              <div v-for="q in QUESTIONS" :key="q.key" class="q-row">
                <span class="q-prompt">{{ q.prompt }}</span>
                <n-input
                  v-model:value="answers[q.key]"
                  :placeholder="q.expect_kind.startsWith('flag') ? 'true / false' : '如 0x19'"
                  style="width: 200px"
                  size="small"
                />
              </div>
            </n-space>
            <template #action>
              <n-button type="primary" size="small" :disabled="busy" @click="submit">提交判题</n-button>
            </template>
          </n-card>

          <!-- 补全 / 修复 / 编程(同一判题模型) -->
          <n-card
            :title="detail.kind === 'fix_bug' ? '修复 Bug' : detail.kind === 'programming' ? '综合编程(真实执行判题)' : '补全指令'"
            size="small" style="margin: 12px 0" v-if="FILLISH"
          >
            <p style="color:#888; margin-top:0">{{ FILLISH.hole_hint }}</p>
            <pre class="asm-view">{{ FILLISH.asm_template.trim() }}</pre>
            <p v-if="detail.kind === 'programming'" style="color:#666; font-size:12px">
              多组用例判题(含边界);通过全部用例才算通过。
            </p>
            <n-input
              v-model:value="answers['hole']"
              :placeholder="detail.kind === 'fix_bug' ? '填入修复后的指令' : '填入一条指令,如 add rax, rbx'"
              size="small"
              style="margin-top: 8px"
            />
            <template #action>
              <n-button type="primary" size="small" :disabled="busy" @click="submit">提交判题(真实执行)</n-button>
            </template>
          </n-card>

          <!-- 静态问答(cpp2asm / debug_challenge) -->
          <n-card title="分析作答" size="small" style="margin: 12px 0" v-if="QUIZ">
            <pre class="asm-view">{{ QUIZ.display.trim() }}</pre>
            <n-space vertical size="medium" style="margin-top: 10px">
              <div v-for="q in QUIZ.questions" :key="q.key" class="q-row">
                <span class="q-prompt">{{ q.prompt }}</span>
                <n-input v-model:value="answers[q.key]" size="small" style="width: 220px" />
              </div>
            </n-space>
            <template #action>
              <n-button type="primary" size="small" :disabled="busy" @click="submit">提交答案</n-button>
            </template>
          </n-card>

          <!-- 自评要点(asm2cpp / explain) -->
          <n-card title="自评要点(全部确认 = 通过)" size="small" style="margin: 12px 0" v-if="SELF">
            <pre v-if="SELF.display.trim()" class="asm-view">{{ SELF.display.trim() }}</pre>
            <div style="margin-top: 10px">
              <div v-for="(pt, i) in SELF.items" :key="i" class="self-row" @click="toggleSelf(i)">
                <span class="self-box" :class="{ on: answers[`item${i}`] === 'true' }">
                  {{ answers[`item${i}`] === 'true' ? '✓' : '' }}
                </span>
                <span>{{ pt }}</span>
              </div>
            </div>
            <p style="color:#666; font-size:12px">诚实模式:先阅读参考解析(提交后在下方),再逐条确认理解。</p>
            <template #action>
              <n-button type="primary" size="small" :disabled="busy" @click="submit">我已理解全部要点</n-button>
            </template>
          </n-card>

          <!-- 判题反馈 -->
          <n-card v-if="result" size="small" style="margin: 12px 0"
                  :title="result.passed ? '✅ 通过' : '❌ 未通过'">
            <n-list show-divider>
              <n-list-item v-for="i in result.items" :key="i.key">
                <n-thing :title="i.prompt">
                  <template #header-extra>
                    <n-tag size="tiny" :type="i.ok ? 'success' : 'error'">{{ i.ok ? '✓' : '✗' }}</n-tag>
                  </template>
                  <span style="color:#888; font-size:13px">
                    你的答案: {{ i.actual }} ·
                    <template v-if="i.ok">正确</template>
                    <template v-else>真实结果: <b style="color:#63e2b7">{{ i.expected }}</b></template>
                  </span>
                </n-thing>
              </n-list-item>
            </n-list>

            <n-alert v-if="result.error" type="error" size="small" style="margin-top:8px">
              {{ result.error }}
            </n-alert>
            <n-alert v-if="result.hint && !result.passed" type="warning" size="small" style="margin-top:8px">
              💡 提示(第 {{ result.attempt_no }} 次尝试): {{ result.hint }}
            </n-alert>
            <n-alert v-if="result.passed" type="success" size="small" style="margin-top:8px">
              通过!判题器使用真实引擎执行验证。返回课程继续下一节。
            </n-alert>
          </n-card>

          <!-- 参考解析(通过后或多次尝试后) -->
          <n-collapse v-if="result?.answer_md" style="margin-bottom: 40px">
            <n-collapse-item title="参考解析" name="ans">
              <div class="answer">{{ result.answer_md }}</div>
            </n-collapse-item>
          </n-collapse>
        </template>
      </n-spin>
    </div>
  </div>
</template>

<style scoped>
.sidebar {
  width: 280px;
  border-right: 1px solid #2a2a2a;
  overflow: auto;
  flex: none;
  padding: 10px;
}
.side-title { color: #888; font-size: 13px; margin: 4px 6px 10px; }
.content { flex: 1; overflow: auto; padding: 20px 28px; }
.ex-head h1 { font-size: 20px; margin: 8px 0; }
.kn { display: flex; gap: 6px; margin-bottom: 8px; }
.q-row { display: flex; align-items: center; gap: 12px; }
.q-prompt { flex: 1; color: #cfcfcf; }
.asm-view {
  background: #111;
  border: 1px solid #2a2a2a;
  border-radius: 4px;
  padding: 10px 14px;
  font-size: 13px;
  line-height: 1.6;
  overflow: auto;
  color: #9ad1a8;
  margin: 0;
  white-space: pre;
}
.answer { white-space: pre-wrap; color: #cfcfcf; line-height: 1.7; }
.self-row { display: flex; gap: 10px; align-items: center; padding: 6px 4px; cursor: pointer; color: #cfcfcf; }
.self-box {
  width: 18px; height: 18px; flex: none; border: 1px solid #3a5a52; border-radius: 3px;
  display: flex; align-items: center; justify-content: center; font-size: 12px; color: #63e2b7;
}
.self-box.on { background: #16241f; border-color: #63e2b7; }
</style>
