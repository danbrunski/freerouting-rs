//! K04.2: lexing allocates nothing per token. A counting global allocator
//! wraps the system one; this file is its own test binary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use freerouting_kicad::sexpr::{Lexer, TokenKind};

struct Counting;
static ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn one_megabyte_of_tokens_lexes_without_allocating() {
    let mut src = String::from("(kicad_pcb\n");
    let mut i = 0u32;
    while src.len() < 1_000_000 {
        src.push_str(&format!(
            "  (segment (start {}.5 -{}.25) (end 1e-3 2) (layer \"F.Cu\") (net \"a(b\\\"c\"))\n",
            i,
            i * 3
        ));
        i += 1;
    }
    src.push(')');
    assert!(src.len() >= 1_000_000);

    let before = ALLOCS.load(Ordering::Relaxed);
    let mut tokens = 0usize;
    let mut strings = 0usize;
    for t in Lexer::new(&src) {
        let t = t.expect("valid input");
        tokens += 1;
        if t.kind == TokenKind::Str {
            strings += 1;
        }
    }
    let after = ALLOCS.load(Ordering::Relaxed);
    assert!(tokens > 200_000, "{tokens} tokens");
    assert_eq!(strings, 2 * i as usize);
    assert_eq!(after - before, 0, "the lexer allocated while lexing");
}
