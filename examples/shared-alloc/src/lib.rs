#![cfg_attr(target_arch = "spirv", no_std)]
#![cfg_attr(target_arch = "spirv", feature(asm_const, asm_experimental_arch))]

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use core::{mem, ptr};
use spirv_std::{RuntimeArray, TypedBuffer};

// HACK(eddyb) normally this would be `u8`, but there is a trade-off where
// using `u8` would require emulating
type HeapUnit = u32;

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

        const HEAP_UNIT_SIZE: usize = mem::size_of::<HeapUnit>();
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
}
