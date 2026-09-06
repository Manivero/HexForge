//! Minimal HexForge WIT component plugin: uppercases ASCII input.
//!
//! Implements the `hexforge:plugin@0.1.0` / `hexforge-plugin` world from
//! `wit/plugin.wit`. The host calls `apply` with fuel + memory limits;
//! any trap is isolated and reported as a plugin error, never a host crash.
//!
//! WASI-free by construction (`#![no_std]` + bump allocator): the host
//! instantiates components with an EMPTY linker, so any WASI import would
//! fail at load. Keep this crate dependency-free (only `wit-bindgen`).
#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicUsize, Ordering};

wit_bindgen::generate!({
    world: "hexforge-plugin",
    path: "wit",
});

use exports::hexforge::plugin::transform::{Capabilities, Guest};

/// Template heap: 16 MiB static buffer. Instances are short-lived (one
/// execution per instance), so never freeing is bounded by a single
/// `apply` call. Raise for bigger payloads; the host caps outputs at
/// 10 MiB regardless.
const HEAP_SIZE: usize = 16 * 1024 * 1024;

static mut HEAP: [u8; HEAP_SIZE] = [0; HEAP_SIZE];
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Bump;

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(1);
        let align = layout.align().max(1);
        let base = core::ptr::addr_of!(HEAP) as usize;
        let mut cur = NEXT.load(Ordering::Relaxed);
        loop {
            let aligned = cur.next_multiple_of(align);
            let Some(end) = aligned.checked_add(size) else {
                return null_mut();
            };
            if end > HEAP_SIZE {
                return null_mut();
            }
            match NEXT.compare_exchange(cur, end, Ordering::SeqCst, Ordering::Relaxed) {
                Ok(_) => return (base + aligned) as *mut u8,
                Err(seen) => cur = seen,
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: Bump = Bump;

#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    // Trap: the host isolates it and reports a plugin error.
    unsafe { core::arch::wasm32::unreachable() }
}

struct Component;

impl Guest for Component {
    fn get_id() -> String {
        String::from("example.wit-uppercase")
    }

    fn get_version() -> String {
        String::from("1.0.0")
    }

    fn get_display_name() -> String {
        String::from("Example WIT Uppercase")
    }

    fn get_category() -> String {
        String::from("Text")
    }

    fn get_params_schema() -> String {
        String::from(r#"{"type":"object","properties":{}}"#)
    }

    fn get_capabilities() -> Capabilities {
        Capabilities {
            deterministic: true,
            streamable: false,
            memory_cost: String::from("full-buffer"),
        }
    }

    fn apply(input: Vec<u8>, _params: String) -> Result<Vec<u8>, String> {
        Ok(input
            .into_iter()
            .map(|b| {
                if b.is_ascii_lowercase() {
                    b - 32
                } else {
                    b
                }
            })
            .collect())
    }
}

export!(Component);
