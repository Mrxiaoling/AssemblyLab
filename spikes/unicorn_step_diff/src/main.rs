// unicorn_step_diff — Assembly Lab Phase 0 spike (NOT product code).
//
// Prove on Windows MSVC host, using the official prebuilt unicorn.dll (2.1.4)
// loaded at runtime (no bindgen / cmake / libclang needed):
//   1. open an x86-64 emulator instance,
//   2. run a hand-assembled program ONE instruction at a time (count=1),
//   3. capture per-instruction before/after register + RFLAGS state,
//   4. capture memory writes through hooks,
//   5. patch registers from the host mid-execution.
//
// This mirrors the "模式 A 指令模拟器" core loop the platform needs.

#![allow(non_snake_case)]

use std::ffi::c_void;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// kernel32 runtime loader
// ---------------------------------------------------------------------------
extern "system" {
    fn LoadLibraryA(name: *const u8) -> *mut c_void;
    fn GetProcAddress(h: *mut c_void, name: *const u8) -> *mut c_void;
}

type uc_engine = c_void;
type uc_hook = u64;
type uc_err = u32;

type CodeHookCb = extern "system" fn(uc: *mut uc_engine, address: u64, size: u32, user_data: *mut c_void);
type MemHookCb = extern "system" fn(uc: *mut uc_engine, mem_type: u32, address: u64, size: u32, value: i64, user_data: *mut c_void) -> bool;

// Constants verified against the official python bindings shipped in the
// unicorn-2.1.4 wheel (unicorn/unicorn_const.py, unicorn/x86_const.py).
const UC_ARCH_X86: u32 = 4;
const UC_MODE_64: u32 = 8;
const UC_ERR_OK: uc_err = 0;
const UC_HOOK_CODE: u32 = 4;
const UC_HOOK_MEM_WRITE: u32 = 2048;
const UC_MEM_WRITE: u32 = 17;
const UC_PROT_ALL: u32 = 7;

// uc_x86_reg ids (alphabetical after the 32-bit block):
const UC_X86_REG_RAX: u32 = 35;
const UC_X86_REG_RBP: u32 = 36;
const UC_X86_REG_RBX: u32 = 37;
const UC_X86_REG_RCX: u32 = 38;
const UC_X86_REG_RDI: u32 = 39;
const UC_X86_REG_RDX: u32 = 40;
const UC_X86_REG_RIP: u32 = 41;
const UC_X86_REG_RSI: u32 = 43;
const UC_X86_REG_RSP: u32 = 44;
const UC_X86_REG_RFLAGS: u32 = 253;

struct UcApi {
    uc_version: unsafe extern "system" fn(*mut u32, *mut u32) -> u32,
    uc_open: unsafe extern "system" fn(u32, u32, *mut *mut uc_engine) -> uc_err,
    uc_close: unsafe extern "system" fn(*mut uc_engine) -> uc_err,
    uc_mem_map: unsafe extern "system" fn(*mut uc_engine, u64, u64, u32) -> uc_err,
    uc_mem_write: unsafe extern "system" fn(*mut uc_engine, u64, *const u8, usize) -> uc_err,
    uc_mem_read: unsafe extern "system" fn(*mut uc_engine, u64, *mut u8, usize) -> uc_err,
    uc_reg_read: unsafe extern "system" fn(*mut uc_engine, u32, *mut c_void) -> uc_err,
    uc_reg_write: unsafe extern "system" fn(*mut uc_engine, u32, *const c_void) -> uc_err,
    uc_emu_start: unsafe extern "system" fn(*mut uc_engine, u64, u64, u64, usize) -> uc_err,
    uc_hook_add: unsafe extern "system" fn(*mut uc_engine, *mut uc_hook, u32, *mut c_void, *mut c_void, u64, u64) -> uc_err,
    uc_errno: unsafe extern "system" fn(*mut uc_engine) -> uc_err,
}

unsafe fn get_sym(h: *mut c_void, name: &str) -> *mut c_void {
    let mut name = format!("{}\0", name);
    let p = GetProcAddress(h, name.as_mut_ptr());
    if p.is_null() {
        panic!("GetProcAddress failed for {}", name.trim_end_matches('\0'));
    }
    p
}

unsafe fn load_api() -> (UcApi, String) {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let path = format!("{}/vendor/unicorn-dist/unicorn.dll\0", manifest);
    let path_display = path.trim_end_matches('\0').to_string();
    let h = LoadLibraryA(path.as_ptr());
    if h.is_null() {
        panic!("LoadLibraryA failed for {}", path_display);
    }
    let api = UcApi {
        uc_version: std::mem::transmute(get_sym(h, "uc_version")),
        uc_open: std::mem::transmute(get_sym(h, "uc_open")),
        uc_close: std::mem::transmute(get_sym(h, "uc_close")),
        uc_mem_map: std::mem::transmute(get_sym(h, "uc_mem_map")),
        uc_mem_write: std::mem::transmute(get_sym(h, "uc_mem_write")),
        uc_mem_read: std::mem::transmute(get_sym(h, "uc_mem_read")),
        uc_reg_read: std::mem::transmute(get_sym(h, "uc_reg_read")),
        uc_reg_write: std::mem::transmute(get_sym(h, "uc_reg_write")),
        uc_emu_start: std::mem::transmute(get_sym(h, "uc_emu_start")),
        uc_hook_add: std::mem::transmute(get_sym(h, "uc_hook_add")),
        uc_errno: std::mem::transmute(get_sym(h, "uc_errno")),
    };
    (api, path_display)
}

// ---------------------------------------------------------------------------
// event capture (hook callbacks run on the emulating thread, synchronously)
// ---------------------------------------------------------------------------
static MEM_WRITES: Mutex<Vec<(u64, u32, i64)>> = Mutex::new(Vec::new());
static CODE_HITS: Mutex<Vec<(u64, u32)>> = Mutex::new(Vec::new());

extern "system" fn on_code(_uc: *mut uc_engine, address: u64, size: u32, _ud: *mut c_void) {
    CODE_HITS.lock().unwrap().push((address, size));
}

extern "system" fn on_mem_write(_uc: *mut uc_engine, mem_type: u32, address: u64, size: u32, value: i64, _ud: *mut c_void) -> bool {
    if mem_type == UC_MEM_WRITE {
        MEM_WRITES.lock().unwrap().push((address, size, value));
    }
    true
}

// ---------------------------------------------------------------------------
// snapshot + diff
// ---------------------------------------------------------------------------
const REGS: [(&str, u32); 10] = [
    ("RAX", UC_X86_REG_RAX),
    ("RBX", UC_X86_REG_RBX),
    ("RCX", UC_X86_REG_RCX),
    ("RDX", UC_X86_REG_RDX),
    ("RSI", UC_X86_REG_RSI),
    ("RDI", UC_X86_REG_RDI),
    ("RBP", UC_X86_REG_RBP),
    ("RSP", UC_X86_REG_RSP),
    ("RIP", UC_X86_REG_RIP),
    ("RFLAGS", UC_X86_REG_RFLAGS),
];

const FLAG_BITS: [(&str, u64); 6] = [
    ("CF", 1 << 0),
    ("PF", 1 << 2),
    ("AF", 1 << 4),
    ("ZF", 1 << 6),
    ("SF", 1 << 7),
    ("OF", 1 << 11),
];

#[derive(Clone, Copy)]
struct Snap([u64; 10]);

unsafe fn snapshot(api: &UcApi, uc: *mut uc_engine) -> Snap {
    let mut v = [0u64; 10];
    for (i, (_, reg)) in REGS.iter().enumerate() {
        let mut x: u64 = 0;
        assert_eq!((api.uc_reg_read)(uc, *reg, &mut x as *mut u64 as *mut c_void), UC_ERR_OK);
        v[i] = x;
    }
    Snap(v)
}

fn hex(v: u64) -> String {
    format!("0x{:X}", v)
}

fn main() {
    unsafe {
        let (api, dll_path) = load_api();
        println!("[*] loaded dll: {}", dll_path);

        let mut major: u32 = 0;
        let mut minor: u32 = 0;
        let ver = (api.uc_version)(&mut major, &mut minor);
        println!("[*] unicorn {}.{} (raw 0x{:X})", major, minor, ver);

        let mut uc: *mut uc_engine = std::ptr::null_mut();
        assert_eq!((api.uc_open)(UC_ARCH_X86, UC_MODE_64, &mut uc), UC_ERR_OK, "uc_open failed");

        const CODE_BASE: u64 = 0x1000;
        const MEM_SIZE: usize = 0x4000;
        assert_eq!((api.uc_mem_map)(uc, CODE_BASE, MEM_SIZE as u64, UC_PROT_ALL), UC_ERR_OK);

        // hand-assembled:
        //   mov eax, 10        ; rax = 10
        //   mov ebx, 20
        //   add rax, rbx       ; rax = 30
        //   mov [0x2000], rax
        //   xor ecx, ecx
        //   sub eax, 5         ; rax = 25
        let code: [u8; 28] = [
            0xB8, 0x0A, 0x00, 0x00, 0x00, // mov eax, 10
            0xBB, 0x14, 0x00, 0x00, 0x00, // mov ebx, 20
            0x48, 0x01, 0xD8, // add rax, rbx
            0x48, 0xA3, 0x00, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // mov [0x2000], rax (moffs64 LE)
            0x31, 0xC9, // xor ecx, ecx
            0x83, 0xE8, 0x05, // sub eax, 5
        ];
        let code_end: u64 = CODE_BASE + code.len() as u64;
        assert_eq!((api.uc_mem_write)(uc, CODE_BASE, code.as_ptr(), code.len()), UC_ERR_OK);

        let rsp: u64 = 0x4800;
        assert_eq!((api.uc_reg_write)(uc, UC_X86_REG_RSP, &rsp as *const u64 as *const c_void), UC_ERR_OK);
        let rip0: u64 = CODE_BASE;
        assert_eq!((api.uc_reg_write)(uc, UC_X86_REG_RIP, &rip0 as *const u64 as *const c_void), UC_ERR_OK);

        let mut hh_code: uc_hook = 0;
        let mut hh_mem: uc_hook = 0;
        assert_eq!(
            (api.uc_hook_add)(uc, &mut hh_code, UC_HOOK_CODE, on_code as *mut c_void, std::ptr::null_mut(), CODE_BASE, code_end),
            UC_ERR_OK
        );
        assert_eq!(
            (api.uc_hook_add)(uc, &mut hh_mem, UC_HOOK_MEM_WRITE, on_mem_write as *mut c_void, std::ptr::null_mut(), 1, 0), // begin>end = hook everywhere
            UC_ERR_OK
        );

        let init = snapshot(&api, uc);
        println!("[*] initial regs: RAX={} RBX={} RSP={}", hex(init.0[0]), hex(init.0[1]), hex(init.0[7]));

        let mut step = 0usize;
        while step < 50 {
            let before = snapshot(&api, uc);
            let rip = before.0[8];
            if rip == code_end || rip == 0 {
                break;
            }
            let err = (api.uc_emu_start)(uc, rip, code_end, 0, 1); // count=1 → exactly one instruction
            step += 1;
            let after = snapshot(&api, uc);

            let mut rdiff = Vec::new();
            for (i, (name, _)) in REGS.iter().enumerate() {
                if before.0[i] != after.0[i] {
                    rdiff.push(format!("{} {}->{}", name, hex(before.0[i]), hex(after.0[i])));
                }
            }

            let fb = before.0[9];
            let fa = after.0[9];
            let mut fdiff = Vec::new();
            for (name, bit) in FLAG_BITS.iter() {
                if (fb & bit) != (fa & bit) {
                    fdiff.push(format!("{}:{}->{}", name, (fb & bit) != 0, (fa & bit) != 0));
                }
            }

            let writes: Vec<(u64, u32, i64)> = MEM_WRITES.lock().unwrap().drain(..).collect();
            let hits: Vec<(u64, u32)> = CODE_HITS.lock().unwrap().drain(..).collect();

            println!("STEP {:>2} @ {}", step, hex(rip));
            for (a, s) in &hits {
                println!("        exec {} size={}", hex(*a), s);
            }
            for (addr, size, val) in &writes {
                println!("        write [{}, {}] = 0x{:X}", hex(*addr), size, *val as u64);
            }
            println!("        regs: {}", if rdiff.is_empty() { "(unchanged)".into() } else { rdiff.join(", ") });
            if !fdiff.is_empty() {
                println!("        flags: {}", fdiff.join(", "));
            }
            if err != UC_ERR_OK {
                println!("        !! emu_start err={} errno={}", err, (api.uc_errno)(uc));
                break;
            }

            // Demo: patch RAX from host after the ADD (proves 修改寄存器 capability)
            if step == 3 {
                let v: u64 = 0x999;
                (api.uc_reg_write)(uc, UC_X86_REG_RAX, &v as *const u64 as *const c_void);
                println!("        ** host patched RAX=0x999 (demo) **");
            }
        }

        let mut out = [0u8; 8];
        assert_eq!((api.uc_mem_read)(uc, 0x2000, out.as_mut_ptr(), 8), UC_ERR_OK);
        let mem_val = u64::from_le_bytes(out);
        let fin = snapshot(&api, uc);

        println!("[*] final: RAX={} RBX={} RCX={} RIP={} RFLAGS=0x{:X}", hex(fin.0[0]), hex(fin.0[1]), hex(fin.0[2]), hex(fin.0[8]), fin.0[9]);
        println!("[*] mem[0x2000] = 0x{:X}", mem_val);

        // after host patch: RAX = 0x999 - 5 = 0x994
        let ok = fin.0[0] == 0x994 && fin.0[1] == 20 && fin.0[2] == 0 && mem_val == 0x999 && step == 6;
        println!("[{}] steps={} (expected 6)", if ok { "PASS" } else { "FAIL" }, step);
    }
}
