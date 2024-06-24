#![cfg_attr(target_arch = "spirv", no_std)]

#[cfg_attr(target_arch = "spirv", global_allocator)]
static _ALLOCATOR: shared_alloc::BumpAllocViaBufs</*DESCRIPTOR_SET*/ 0> =
    shared_alloc::BumpAllocViaBufs;

extern crate alloc;

use alloc::boxed::Box;

use spirv_std::glam::UVec3;
use spirv_std::spirv;

#[spirv(compute(threads(128)))]
pub fn box_new_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 4 == 0 {
        let _ = Box::new(id.x / 4);
    }
}