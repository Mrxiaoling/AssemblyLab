<script setup lang="ts">
// 解释卡:当前步的 before/after + 指令说明(数据全部来自后端真实执行)
import { computed } from 'vue'
import { NTag } from 'naive-ui'
import { useSessionStore } from '../../stores/session'

const store = useSessionStore()
const report = computed(() => store.lastReports[store.lastReports.length - 1] ?? null)
</script>

<template>
  <div class="panel" style="height: 100%">
    <div class="panel-title">指令解释(当前步)</div>
    <div class="panel-body card">
      <div v-if="!report" style="color: #555; padding: 12px">单步执行后,这里展示每条指令执行前后的状态与解释。</div>

      <template v-else>
        <div class="head">
          <span class="addr">{{ report.address }}</span>
          <span class="bytes">{{ report.machine_bytes_hex }}</span>
          <span class="dis">{{ report.disasm }}</span>
          <n-tag size="tiny" :bordered="false" type="info">{{ report.explanation.category || '指令' }}</n-tag>
        </div>

        <div class="section">
          <div class="sec-title">执行前后</div>
          <table class="diff">
            <tbody>
              <tr v-for="d in report.reg_diff" :key="d.name">
                <td class="d-name">{{ d.name }}</td>
                <td class="d-before">{{ d.before }}</td>
                <td class="d-arrow">→</td>
                <td class="d-after">{{ d.after }}</td>
                <td v-if="d.dec" class="d-dec">({{ d.dec[0] }} → {{ d.dec[1] }})</td>
              </tr>
              <tr v-for="f in report.flag_diff" :key="'f' + f.name">
                <td class="d-name">FLAG {{ f.name }}</td>
                <td class="d-before">{{ f.before ? 1 : 0 }}</td>
                <td class="d-arrow">→</td>
                <td class="d-after">{{ f.after ? 1 : 0 }}</td>
                <td class="d-dec">{{ f.semantics }}</td>
              </tr>
              <tr v-for="m in report.mem_changes" :key="'m' + m.addr">
                <td class="d-name">[{{ m.addr }}]</td>
                <td class="d-before" colspan="2">写 0x{{ m.value_hex }}</td>
                <td class="d-dec" colspan="2">{{ m.size }} 字节 · {{ m.region }}</td>
              </tr>
            </tbody>
          </table>
          <div v-if="report.reg_diff.length === 0 && report.flag_diff.length === 0 && report.mem_changes.length === 0" class="none">
            (本条指令未改变可见状态)
          </div>
        </div>

        <div class="section">
          <div class="sec-title">这条指令做了什么</div>
          <p class="summary">{{ report.explanation.summary }}</p>
          <ul v-if="report.explanation.detail.length" class="detail">
            <li v-for="(d, i) in report.explanation.detail" :key="i">{{ d }}</li>
          </ul>
          <p v-if="report.explanation.related_cpp" class="cpp">
            ↔ C++: {{ report.explanation.related_cpp }}
          </p>
          <div v-for="(p, i) in report.explanation.pitfalls" :key="'p' + i" class="pitfall">
            ⚠ {{ p }}
          </div>
          <div v-for="(w, i) in report.explanation.warnings" :key="'w' + i" class="warning">
            ✱ {{ w }}
          </div>
        </div>

        <div class="section">
          <div class="sec-title">程序计数器</div>
          <p class="summary">
            RIP: {{ report.pc_jump.from }} → {{ report.pc_jump.to }}
            <n-tag size="tiny" :bordered="false">
              {{ { seq: '顺序执行', branch: '跳转', call: '调用', ret: '返回', syscall: '系统调用' }[report.pc_jump.kind] }}
            </n-tag>
          </p>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.card {
  padding: 8px 12px;
  font-size: 12.5px;
}
.head {
  display: flex;
  gap: 10px;
  align-items: baseline;
  padding: 4px 0;
  border-bottom: 1px solid #2a2a2a;
}
.addr {
  color: #7ec699;
}
.bytes {
  color: #555;
}
.dis {
  color: #fff;
  font-size: 14px;
}
.section {
  padding: 6px 0;
  border-bottom: 1px dashed #2a2a2a;
}
.sec-title {
  color: #666;
  font-size: 11px;
  margin-bottom: 4px;
}
.diff {
  border-collapse: collapse;
  width: 100%;
}
.d-name {
  color: #82aaff;
  padding-right: 8px;
  white-space: nowrap;
}
.d-before {
  color: #777;
  padding-right: 6px;
}
.d-arrow {
  color: #555;
  padding-right: 6px;
}
.d-after {
  color: #ffc83c;
  padding-right: 6px;
}
.d-dec {
  color: #555;
  font-size: 11px;
}
.none {
  color: #555;
  padding: 4px 0;
}
.summary {
  margin: 2px 0;
  color: #cfcfcf;
}
.detail {
  margin: 4px 0 0;
  padding-left: 18px;
  color: #9ad1a8;
}
.cpp {
  color: #82aaff;
  margin: 6px 0 0;
}
.pitfall {
  color: #f0a35e;
  margin-top: 4px;
}
.warning {
  color: #ff6b6b;
  margin-top: 4px;
}
</style>
