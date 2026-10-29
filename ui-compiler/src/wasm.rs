//! The C ABI of `dist/ui-compiler.wasm` (`--features wasm`). The host
//! `alloc`s room for the name and the source, writes them, calls
//! `compile(name_ptr, name_len, src_ptr, src_len)`, and reads the answer —
//! JSON `{ ok } | { err }` — at the pointer and length packed in the result
//! (`ptr << 32 | len`); then it `free`s all three. Nothing is imported: the
//! module reaches nothing but its own memory.

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

/// Compile the source at `src_ptr..+src_len` as the name at
/// `name_ptr..+name_len`; the answer's pointer and length, packed
/// `ptr << 32 | len`. A name or a source that is not UTF-8 is refused at
/// line 0.
#[no_mangle]
pub extern "C" fn compile(name_ptr: u32, name_len: u32, src_ptr: u32, src_len: u32) -> u64 {
    let name = unsafe { core::slice::from_raw_parts(name_ptr as usize as *const u8, name_len as usize) };
    let src = unsafe { core::slice::from_raw_parts(src_ptr as usize as *const u8, src_len as usize) };
    let json = match (core::str::from_utf8(name), core::str::from_utf8(src)) {
        (Ok(name), Ok(src)) => crate::compile_json(name, src),
        (Err(_), _) => crate::refusal_json(crate::Refusal { line: 0, message: "the name is not UTF-8".into() }),
        (_, Err(_)) => crate::refusal_json(crate::Refusal { line: 0, message: "the source is not UTF-8".into() }),
    };
    let out = json.into_bytes();
    let n = out.len() as u32;
    let p = alloc(n);
    unsafe { core::ptr::copy_nonoverlapping(out.as_ptr(), p as usize as *mut u8, out.len()) };
    ((p as u64) << 32) | n as u64
}
