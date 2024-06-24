#![cfg_attr(target_arch = "spirv", no_std)]
#![cfg_attr(target_arch = "spirv", feature(asm_const, asm_experimental_arch))]

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
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
    unsafe fn ptr_to_storage_buffer<const BINDING: u32, T>() -> &'static TypedBuffer<UnsafeCell<T>>
    {
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
    unsafe fn ptr_to_heap_buffer() -> *mut RuntimeArray<HeapUnit> {
        Self::ptr_to_storage_buffer::<0, _>().get()
    }
    unsafe fn ptr_to_remaining_atomic_buffer() -> *mut u32 {
        Self::ptr_to_storage_buffer::<1, _>().get()
    }
}

// FIXME(eddyb) kind of nonsensical, but specifically for logical buffers,
// alignment isn't actually relevant, as legalization has to find some way
// of representing all interactions with the buffers as plain arrays anyway.
const MAX_SUPPORTED_ALIGN: usize = 1 << 31;

// HACK(eddyb) like `Atomic*::fetch_update(Relaxed, Relaxed, ...)`,
// but for the lower-level `spirv_std::arch::atomic_*`.
unsafe fn atomic_fetch_update_relaxed_relaxed<T>(
    ptr: *mut T,
    mut f: impl FnMut(T) -> Option<T>,
) -> Result<T, T>
where
    T: spirv_std::integer::Integer + spirv_std::number::Number,
{
    // FIXME(eddyb) creating `&T` is wrong for atomics (implies unsharing).
    let mut prev = spirv_std::arch::atomic_load::<
        _,
        { spirv_std::memory::Scope::Device as u32 },
        { spirv_std::memory::Semantics::NONE.bits() as u32 },
    >(&*ptr);
    while let Some(next) = f(prev) {
        // FIXME(eddyb) creating `&mut T` is wrong for atomics (implies unsharing).
        let next_prev = spirv_std::arch::atomic_compare_exchange::<
            _,
            { spirv_std::memory::Scope::Device as u32 },
            { spirv_std::memory::Semantics::NONE.bits() as u32 },
            { spirv_std::memory::Semantics::NONE.bits() as u32 },
        >(&mut *ptr, next, prev);
        if next_prev == prev {
            return Ok(prev);
        }
        prev = next_prev;
    }
    Err(prev)
}

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
        if atomic_fetch_update_relaxed_relaxed(
            Self::ptr_to_remaining_atomic_buffer(),
            |remaining| {
                let mut remaining = remaining as usize;
                if size > remaining {
                    return None;
                }
                remaining -= size;
                remaining &= align_mask_to_round_down;
                allocated = remaining;
                Some(remaining as u32)
            },
        )
        .is_err()
        {
            return ptr::null_mut();
        }
        Self::ptr_to_heap_buffer()
            .cast::<HeapUnit>()
            .add(allocated)
            .cast::<u8>()
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
