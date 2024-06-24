#![cfg_attr(target_arch = "spirv", no_std)]

use spirv_std::spirv;

#[spirv(fragment)]
pub fn slice_iter(
    #[spirv(uniform, descriptor_set = 0, binding = 0)] data: &[u32],
    output: &mut u32,
) {
    *output = data.iter().sum();
}
