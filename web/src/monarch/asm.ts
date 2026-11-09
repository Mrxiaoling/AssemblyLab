// x86-64 NASM 语法的 Monaco 高亮定义(自定义 Monarch 规则)
import type { languages } from 'monaco-editor'

const REGS = [
  'rax','rbx','rcx','rdx','rsi','rdi','rbp','rsp',
  'r8','r9','r10','r11','r12','r13','r14','r15',
  'eax','ebx','ecx','edx','esi','edi','ebp','esp',
  'r8d','r9d','r10d','r11d','r12d','r13d','r14d','r15d',
  'ax','bx','cx','dx','si','di','bp','sp',
  'r8w','r9w','r10w','r11w','r12w','r13w','r14w','r15w',
  'al','bl','cl','dl','sil','dil','bpl','spl',
  'r8b','r9b','r10b','r11b','r12b','r13b','r14b','r15b',
  'ah','bh','ch','dh',
  'rip','eip','rflags','eflags','flags',
  'xmm0','xmm1','xmm2','xmm3','xmm4','xmm5','xmm6','xmm7',
  'xmm8','xmm9','xmm10','xmm11','xmm12','xmm13','xmm14','xmm15',
]

const MNEMONICS = [
  'mov','movzx','movsx','movsxd','lea','push','pop',
  'add','adc','sub','sbb','cmp','test','inc','dec','neg',
  'mul','imul','div','idiv',
  'and','or','xor','not',
  'shl','shr','sar','rol','ror','rcl','rcr',
  'jmp','je','jne','jz','jnz','js','jns','jc','jnc','jo','jno',
  'jb','jbe','ja','jae','jl','jle','jg','jge','jp','jnp','jcxz','jecxz','jrcxz',
  'loop','loope','loopne','call','ret','leave','enter',
  'nop','hlt','int','syscall','sysenter','cpuid','xorpd','xorps',
  'movaps','movups','movdqa','movdqu','paddd','paddq','pmulld','movd','movq',
  'stc','clc','cmc','std','cld','cbw','cwde','cdqe','cqo','cwd','cdq',
  'sete','setne','setz','setnz','setl','setg','setle','setge',
  'lock','xadd','cmpxchg','xchg','bswap','mfence','lfence','sfence',
]

export const asmLanguageId = 'x86-nasm'

export async function registerAsmLanguage(): Promise<void> {
  const monaco = await import('monaco-editor')
  if (monaco.languages.getLanguages().some((l) => l.id === asmLanguageId)) return

  monaco.languages.register({ id: asmLanguageId })
  monaco.languages.setMonarchTokensProvider(asmLanguageId, {
    ignoreCase: true,
    tokenizer: {
      root: [
        // 注释
        [/;.*$/, 'comment'],
        // 段/伪指令
        [/^\s*\[[a-z]+\]/, 'keyword'],
        [/^\s*(org|section|segment|global|extern|bits|default|align|times)\b/, 'keyword'],
        // 标签定义(label:)
        [/^[A-Za-z_.$?][\w.$?#@~]*\s*:/, 'type.identifier'],
        // 寄存器
        [new RegExp(`\\b(?:${REGS.join('|')})\\b`), 'variable.predefined'],
        // 十六进制 / 二进制 / 十进制
        [/\b0[xX][0-9a-fA-F]+\b/, 'number.hex'],
        [/\b[0-9a-fA-F]+h\b/, 'number.hex'],
        [/\b[01]+b\b/, 'number.binary'],
        [/\b\d+(\.\d+)?\b/, 'number'],
        // 字符/字符串
        [/"([^"\\]|\\.)*"/, 'string'],
        [/'([^'\\]|\\.)*'/, 'string'],
        // 助记符
        [new RegExp(`\\b(?:${MNEMONICS.join('|')})\\b`), 'keyword.mnemonic'],
        // 局部标号引用
        [/\.[\w.$?]+/, 'type.identifier'],
      ],
    },
  } as languages.IMonarchLanguage)
}
