<script setup lang="ts">
// 内存/栈查看器(P3 类型化升级):
//   字节视图(hex+ASCII,可编辑)+ 类型化视图(字长/进制/端序/浮点)
//   按 RSP/RBP 快速跳栈;步进后自动跟随
import { computed, onMounted, ref, watch } from 'vue'
import { NButton, NInput, NTag, NSelect } from 'naive-ui'
import { api } from '../../api/client'
import type { MemoryResp } from '../../api/types'
import { useSessionStore } from '../../stores/session'
import { decodeValues, chunkCells, MEM_TYPES, typeSize, type MemType } from '../../utils/memdecode'

const store = useSessionStore()

const addrInput = ref('0x400000')
const view = ref<MemoryResp | null>(null)
const editing = ref(false)
const editHex = ref('')

// 类型化视图状态
const viewType = ref<MemType>('byte')
const endian = ref<'le' | 'be'>('le')
const radix = ref<'hex' | 'dec' | 'bin'>('dec')

const typeOptions = MEM_TYPES.map((t) => ({ label: t.label, value: t.value }))
const endianOptions = [
  { label: '小端 (x86)', value: 'le' },
  { label: '大端 (对照)', value: 'be' },
]
const radixOptions = [
  { label: '十进制', value: 'dec' },
  { label: '十六进制', value: 'hex' },
  { label: '二进制', value: 'bin' },
]

async function load() {
  if (!store.sessionId) return
  try {
    view.value = await api.memory(store.sessionId, addrInput.value, 256)
  } catch (e) {
    view.value = null
    store.error = e instanceof Error ? e.message : String(e)
  }
}

async function goReg(name: 'RSP' | 'RBP') {
  const v = store.regMap.get(name)
  if (v) {
    addrInput.value = v
    await load()
  }
}

async function startEdit() {
  if (!view.value) return
  editHex.value = view.value.hex
  editing.value = true
}

async function commitEdit() {
  if (!store.sessionId || !view.value) return
  try {
    await api.patchMemory(store.sessionId, view.value.addr, editHex.value.replace(/\s+/g, ''))
    editing.value = false
    await load()
    await store.refresh()
  } catch (e) {
    store.error = e instanceof Error ? e.message : String(e)
  }
}

function byteRows(): { off: string; hex: string[]; ascii: string }[] {
  if (!view.value) return []
  const hex = view.value.hex
  const ascii = view.value.ascii
  const out: { off: string; hex: string[]; ascii: string }[] = []
  for (let i = 0; i < hex.length; i += 32) {
    const chunk: string[] = []
    for (let j = i; j < Math.min(i + 32, hex.length); j += 2) chunk.push(hex.slice(j, j + 2))
    const base = BigInt(view.value.addr) + BigInt(i / 2)
    out.push({
      off: '0x' + base.toString(16).toUpperCase(),
      hex: chunk,
      ascii: ascii.slice(i / 2, i / 2 + 16),
    })
  }
  return out
}

// 类型化行:每行放 8 个单元(8 字节类型放 4 个)
const typedRows = computed(() => {
  if (!view.value || viewType.value === 'byte') return []
  const perRow = typeSize(viewType.value) >= 8 ? 4 : 8
  return chunkCells(decodeValues(view.value.hex, viewType.value, endian.value === 'le', radix.value), perRow)
})

const typedOffWidth = computed(() => (typeSize(viewType.value) >= 8 ? 8 : 4))

// 步进后自动重载当前视图
watch(
  () => store.state?.step_count,
  async () => {
    await load()
  },
)
onMounted(load)
</script>

<template>
  <div class="panel" style="height: 100%">
    <div class="panel-title">
      内存
      <n-tag :type="view?.region === 'unmapped' ? 'error' : 'info'" size="tiny" :bordered="false">
        {{ view?.region ?? '—' }}
      </n-tag>
    </div>
    <div class="toolbar">
      <n-input v-model:value="addrInput" size="tiny" placeholder="0x400000" @keyup.enter="load" />
      <n-button size="tiny" @click="load">查看</n-button>
      <n-button size="tiny" @click="goReg('RSP')">RSP</n-button>
      <n-button size="tiny" @click="goReg('RBP')">RBP</n-button>
    </div>
    <div class="toolbar">
      <n-select v-model:value="viewType" size="tiny" :options="typeOptions" style="width: 110px" />
      <template v-if="viewType !== 'byte'">
        <n-select v-model:value="endian" size="tiny" :options="endianOptions" style="width: 110px" />
        <n-select
          v-if="viewType.startsWith('i') || viewType.startsWith('u')"
          v-model:value="radix"
          size="tiny"
          :options="radixOptions"
          style="width: 92px"
        />
      </template>
      <div style="flex: 1" />
      <template v-if="viewType === 'byte'">
        <n-button v-if="!editing" size="tiny" @click="startEdit">编辑</n-button>
        <template v-else>
          <n-input v-model:value="editHex" size="tiny" style="width: 200px" />
          <n-button size="tiny" type="primary" @click="commitEdit">写入</n-button>
          <n-button size="tiny" @click="editing = false">取消</n-button>
        </template>
      </template>
    </div>
    <div class="panel-body">
      <!-- 字节视图 -->
      <table v-if="view && viewType === 'byte'" class="mem">
        <tbody>
          <tr v-for="r in byteRows()" :key="r.off">
            <td class="m-off">{{ r.off }}</td>
            <td class="m-hex">
              <span v-for="(b, i) in r.hex" :key="i" :class="{ z: b === '00' }">{{ b }}</span>
            </td>
            <td class="m-ascii">{{ r.ascii }}</td>
          </tr>
        </tbody>
      </table>
      <!-- 类型化视图 -->
      <table v-else-if="view" class="mem typed">
        <tbody>
          <tr v-for="(row, ri) in typedRows" :key="ri">
            <td class="m-off">+{{ (row[0].off).toString(16).toUpperCase().padStart(typedOffWidth, '0') }}</td>
            <td v-for="c in row" :key="c.off" class="m-val" :class="{ z: /^0x0+$/.test(c.text) || c.text === '0' }">
              {{ c.text }}
            </td>
          </tr>
        </tbody>
      </table>
      <div v-else style="color: #555; padding: 10px">输入地址查看内存</div>
    </div>
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  gap: 6px;
  padding: 4px 10px;
  align-items: center;
  flex: none;
  border-bottom: 1px solid #222;
}
.mem {
  font-size: 12px;
  border-collapse: collapse;
  width: 100%;
}
.m-off {
  color: #666;
  padding-right: 10px;
  white-space: nowrap;
}
.m-hex span {
  display: inline-block;
  width: 22px;
  color: #cfcfcf;
}
.m-hex span.z {
  color: #4a4a4a;
}
.m-ascii {
  color: #7ec699;
  white-space: nowrap;
}
.typed .m-val {
  padding: 0 8px 0 0;
  color: #cfcfcf;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}
.typed .m-val.z {
  color: #4a4a4a;
}
</style>
