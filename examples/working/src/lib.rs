#![cfg_attr(target_arch = "spirv", no_std)]

#[cfg_attr(target_arch = "spirv", global_allocator)]
static _ALLOCATOR: shared_alloc::BumpAllocViaBufs</*DESCRIPTOR_SET*/ 0> =
    shared_alloc::BumpAllocViaBufs;

#[macro_use]
extern crate alloc;

use alloc::boxed::Box;
use alloc::rc::Rc;

use spirv_std::glam::UVec3;
use spirv_std::spirv;

#[cfg(DISABLED)]
#[spirv(compute(threads(128)))]
pub fn btree_set_new_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 4 == 0 {
        let _ = alloc::collections::BTreeSet::<u32>::new();
    }
}
#[spirv(compute(threads(128)))]
pub fn btree_map_new_u32_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 4 == 0 {
        let mut map = alloc::collections::BTreeMap::<u32, u32>::new();
        map.insert(0, 0);
        core::mem::forget(map);
    }
}

#[cfg(DISABLED)]
#[spirv(compute(threads(128)))]
pub fn box_new_box_new_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 4 == 0 {
        let _ = Box::new(Box::new(id.x));
    }
}

#[cfg(DISABLED)]
#[spirv(compute(threads(128)))]
pub fn box_or_vec_1_u32(#[spirv(global_invocation_id)] id: UVec3) {
    match id.x % 8 {
        0 => {
            let _ = Box::new(id.x);
        }
        1 => {
            let _ = vec![id.x];
        }
        _ => {}
    }
}

#[cfg(DISABLED)]
#[spirv(compute(threads(128)))]
pub fn box_new_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 4 == 0 {
        let _ = Box::new(id.x);
    }
}

#[cfg(DISABLED)]
// FIXME(eddyb) incorporate into the big example, and `README`.
#[spirv(compute(threads(128)))]
pub fn rc_new_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 4 == 0 {
        let _ = Rc::new(id.x);
    }
}

#[cfg(DISABLED)]
#[spirv(compute(threads(128)))]
pub fn vec_1_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 4 == 0 {
        let _ = vec![id.x];
    }
}

#[cfg(DISABLED)]
// FIXME(eddyb) incorporate into the big example, and `README`.
#[spirv(compute(threads(128)))]
pub fn vec_2_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 4 == 0 {
        let mut v = vec![id.x, id.x];
        v[(id.x / 4) as usize % 2] |= 0xabcd_0000;
    }
}

#[spirv(compute(threads(128)))]
#[cfg(ACTUALLY_BROKEN_OOPS)]
pub fn vec_new_push_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 4 == 0 {
        let mut v = vec![];
        v.push(id.x);
    }
}

// NOTE(eddyb) this forces `realloc` to trigger with 1 `push` call and 0 loops.
#[spirv(compute(threads(128)))]
#[cfg(ACTUALLY_BROKEN_OOPS)]
pub fn vec_cap1_push_u32(#[spirv(global_invocation_id)] id: UVec3) {
    if id.x % 16 == 0 {
        let mut v = vec![id.x];
        v.push(id.x | 0x1111_0000);
    }
}
