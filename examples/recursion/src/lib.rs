#![cfg_attr(target_arch = "spirv", no_std)]

use spirv_std::glam::UVec3;
use spirv_std::spirv;

fn factorial(n: u32) -> u32 {
    if n < 2 {
        1
    } else {
        factorial(n - 1) * n
    }
}

fn fib(n: u32) -> u32 {
    if n < 2 {
        n
    } else {
        fib(n - 2) + fib(n - 1)
    }
}

// HACK(eddyb) testing two non-mutual callgraph cycles.
fn sum_fib(n: u32) -> u32 {
    if n == 0 {
        0
    } else {
        sum_fib(n - 1) + fib(n)
    }
}

#[spirv(compute(threads(128)))]
pub fn test_factorial(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 0)] output: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] output_unused_prefix: &mut usize,
) {
    let start = output.len() - 128;
    *output_unused_prefix = start;
    // HACK(eddyb) just in case of panic.
    output[start + id.x as usize] = !0;
    output[start + id.x as usize] = factorial(id.x + 3);
}

#[spirv(compute(threads(24)))]
pub fn test_fib(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 0)] output: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] output_unused_prefix: &mut usize,
) {
    let start = output.len() - 24;
    *output_unused_prefix = start;
    output[start + id.x as usize] = fib(id.x);
}

// HACK(eddyb) propagating stack overflows isn't possible yet for this example.
#[cfg(DISABLED)]
#[spirv(compute(threads(24)))]
pub fn test_sum_fib(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 0)] output: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] output_unused_prefix: &mut usize,
) {
    let start = output.len() - 24;
    *output_unused_prefix = start;
    output[start + id.x as usize] = sum_fib(id.x);
}

#[test]
fn test_sum_fib_first_24() {
    let outputs_first_24 = [
        0x00000000, 0x00000001, 0x00000002, 0x00000004, 0x00000007, 0x0000000c, 0x00000014,
        0x00000021, 0x00000036, 0x00000058, 0x0000008f, 0x000000e8, 0x00000178, 0x00000261,
        0x000003da, 0x0000063c, 0x00000a17, 0x00001054, 0x00001a6c, 0x00002ac1, 0x0000452e,
        0x00006ff0, 0x0000b51f, 0x00012510,
    ];
    for (i, o) in outputs_first_24.into_iter().enumerate() {
        assert_eq!(sum_fib(i as u32), o);
    }
}
