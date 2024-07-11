#![cfg_attr(target_arch = "spirv", no_std)]
#![cfg_attr(target_arch = "spirv", feature(asm_const, asm_experimental_arch))]

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use core::{mem, ptr};
use spirv_std::{RuntimeArray, TypedBuffer};

// HACK(eddyb) normally this would be `u8`, but there is a trade-off where
// using `u8` would require emulating e.g. `u32` accesses with 4 `u8` ones.
// FIXME(eddyb) at least for `StorageBuffer` (and `Workgroup` with an extension),
// SPIR-V allows multiple declarations with the same `DescriptorSet`/`Binding`
// decorations, to perform type punning, the main caveats being:
// - `Aliased` decorations are required on all such declarations to not be UB
//   (but are not fine-grained enough to express disjointness between buffers
//    coming from different bindings - OTOH Vulkan also allows those to overlap,
//    so perhaps `Alias` should be the default in Rust-GPU and require opt-out?)
// - SPIR-T would likely need to stop using "global variables" to model resources,
//   and instead have special constant forms for "pointer to resource binding"
//   (though constants aren't the right idea for *resources*, so maybe each
//    entry-point should have its own argument, or at least special global var,
//    that acts as e.g. a `Handles` address space base pointer, which arguably
//    is similar to Metal "argument buffers" or Vulkan "descriptor buffers")
type HeapUnit = u32;
const HEAP_UNIT_SIZE: usize = mem::size_of::<HeapUnit>();

pub struct BumpAllocViaBufs<const DESCRIPTOR_SET: u32>;

// HACK(eddyb) to avoid having to progate dataflow through mutable globals,
// accessing the buffers used for the "heap" is hardcoded via `asm!`, and
// using `TypedBuffer` to get (indirect) access to "interface block" types.
impl<const DESCRIPTOR_SET: u32> BumpAllocViaBufs<DESCRIPTOR_SET> {
    #[spirv_std::macros::gpu_only]
    unsafe fn storage_buffer<const BINDING: u32, T>() -> &'static TypedBuffer<T> {
        // FIXME(eddyb) it would be useful if this "slot" wasn't needed.
        let mut result_slot = mem::MaybeUninit::uninit();
        core::arch::asm!(
            "OpDecorate %var DescriptorSet {descriptor_set}",
            "OpDecorate %var Binding {binding}",
            "%var = OpVariable typeof*{result_slot} StorageBuffer",
            "OpStore {result_slot} %var",
            descriptor_set = const DESCRIPTOR_SET,
            binding = const BINDING,
            result_slot = in(reg) result_slot.as_mut_ptr(),
        );
        result_slot.assume_init()
    }
    unsafe fn heap_buffer() -> &'static UnsafeCell<RuntimeArray<HeapUnit>> {
        Self::storage_buffer::<0, UnsafeCell<_>>()
    }
    unsafe fn remaining_atomic_buffer() -> &'static AtomicUsize {
        Self::storage_buffer::<1, AtomicUsize>()
    }
}

// FIXME(eddyb) kind of nonsensical, but specifically for logical buffers,
// alignment isn't actually relevant, as legalization has to find some way
// of representing all interactions with the buffers as plain arrays anyway.
const MAX_SUPPORTED_ALIGN: usize = 1 << 31;

unsafe impl<const DESCRIPTOR_SET: u32> GlobalAlloc for BumpAllocViaBufs<DESCRIPTOR_SET> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() > MAX_SUPPORTED_ALIGN {
            return ptr::null_mut();
        }

        let size = (layout.size() + (HEAP_UNIT_SIZE - 1)) / HEAP_UNIT_SIZE;
        let align = (layout.align() + (HEAP_UNIT_SIZE - 1)) / HEAP_UNIT_SIZE;

        // `Layout` contract forbids making a `Layout` with align=0, or align not power of 2.
        // So we can safely use a mask to ensure alignment without worrying about UB.
        let align_mask_to_round_down = !(align - 1);

        let mut allocated = 0;
        if Self::remaining_atomic_buffer()
            .fetch_update(Relaxed, Relaxed, |mut remaining| {
                if size > remaining {
                    return None;
                }
                remaining -= size;
                remaining &= align_mask_to_round_down;
                allocated = remaining;
                Some(remaining)
            })
            .is_err()
        {
            return ptr::null_mut();
        }
        Self::heap_buffer()
            .get()
            .cast::<HeapUnit>()
            .add(allocated)
            .cast::<u8>()
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}

    // HACK(eddyb) explicitly implemented to rely on `HeapUnit` and avoid `memcpy`.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };
        let new_ptr = unsafe { self.alloc(new_layout) };
        if !new_ptr.is_null() {
            unsafe {
                // HACK(eddyb) copying one `HeapUnit` at a time and not keeping
                // pointers as "loop state" simplifies analysis/legalization.
                // NOTE(eddyb) for reference, the `realloc` method defaults to:
                //   ptr::copy_nonoverlapping(ptr, new_ptr, cmp::min(layout.size(), new_size));
                let copy_size_in_heap_units = (core::cmp::min(layout.size(), new_size)
                    + (HEAP_UNIT_SIZE - 1))
                    / HEAP_UNIT_SIZE;
                for i in 0..copy_size_in_heap_units {
                    let dst = new_ptr.cast::<HeapUnit>().add(i);
                    let src = ptr.cast::<HeapUnit>().add(i);
                    // HACK(eddyb) `dst.copy_from_nonoverlapping(src, 1)` is the
                    // normal way to do this, but even that doesn't work for some
                    // reason (and even if it did, it would do exactly the same
                    // load+store pair, as sadly there are no "raw bytes" types).
                    *dst = *src;
                }
                self.dealloc(ptr, layout);
            }
        }
        new_ptr
    }
}
