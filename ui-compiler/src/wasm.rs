//! The C ABI of `dist/ui-compiler.wasm` (`--features wasm`). The host
//! `alloc`s `len` bytes, writes the source there, calls `compile(ptr, len)`,
//! and reads the answer — JSON `{ ok } | { err }` — at the pointer and length
//! packed in the result (`ptr << 32 | len`); then it `free`s both. Nothing
//! is imported: the module reaches nothing but its own memory.

use std::alloc::{alloc as raw_alloc, dealloc, Layout};

/// `len` bytes for the host to write into; never null (a zero length gets 1).
#[no_mangle]
pub extern "C" fn alloc(len: u32) -> u32 {
    let layout = Layout::from_size_align(len.max(1) as usize, 1).expect("a byte layout");
    unsafe { raw_alloc(layout) as usize as u32 }
}

/// Give back what `alloc` (or `compile`'s answer) handed out.
#[no_mangle]
pub extern "C" fn free(ptr: u32, len: u32) {
    let layout = Layout::from_size_align(len.max(1) as usize, 1).expect("a byte layout");
    unsafe { dealloc(ptr as usize as *mut u8, layout) }
}

/// Compile the source at `ptr..ptr+len`; the answer's pointer and length,
/// packed `ptr << 32 | len`. A source that is not UTF-8 is refused at line 0.
#[no_mangle]
pub extern "C" fn compile(ptr: u32, len: u32) -> u64 {
    let bytes = unsafe { core::slice::from_raw_parts(ptr as usize as *const u8, len as usize) };
    let json = match core::str::from_utf8(bytes) {
        Ok(src) => crate::compile_json(src),
        Err(_) => serde_json::to_string(&crate::Answer::Err(crate::Refusal {
            line: 0,
            message: "the source is not UTF-8".into(),
        }))
        .expect("an answer always serialises"),
    };
    let out = json.into_bytes();
    let n = out.len() as u32;
    let p = alloc(n);
    unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), p as usize as *mut u8, out.len()) };
    ((p as u64) << 32) | n as u64
}
