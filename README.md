# Pointers in GPU shaders: _are we `alloc` yet?_<br><sup><sub>_(showcasing a Rust-GPU experimental branch)_</sub></sup>

## What?

Excerpt from [`examples/working/src/lib.rs`](examples/working/src/lib.rs):
```rust
#[global_allocator] // (details elided, see original file)
static ALLOCATOR: ... = ...;

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
```
```console
$ cargo run --release examples/working
...
box_or_vec_1_u32: allocated 128 bytes in 22.666µs, leaving this heap behind:
  00000039 00000029 00000019 00000009 00000031 00000079 00000069 00000059
  00000049 00000071 00000061 00000051 00000041 00000021 00000078 00000068
  00000058 00000048 00000070 00000060 00000050 00000011 00000001 00000040
  00000038 00000028 00000018 00000008 00000030 00000020 00000010 00000000
```
A few details of note in the above output (i.e. the "heap dump"):
- it appears reversed because the example `#[global_allocator]` grows downwards
- all 32 invocations chosen to perform `Box::new`/`vec![...]` did so successfully
- having 128 invocations in total is used to reveal some interleaving,
  as invocations from different subgroups are (atomically) competing
  (the exact pattern will likely vary between GPUs/drivers/etc.)

### Is it safe?

OOM detection/reporting (via Rust-GPU's `panic!` emulation) should work,
i.e. running out of the CPU-provided storage buffer for the heap will safely abort,
instead of causing any memory corruption (or generally undefined behavior).

_(**TODO**: demo that by enabling verbose `debugPrintf` for panics)_

### Is that it?

These Rust-GPU/SPIR-T changes are actively being worked on, having started with
a minimal `Box::new(123)` (plus gnarlier underlying `alloc` abstractions) being
the first example to successfully compile, and continuing towards increasingly
more difficult ones (also see `examples/broken-*`, as likely to be tackled next).

As support expands, the examples and README in this repository will be updated
to reflect that progress (ideally culminating in some future Rust-GPU release).

## Why?

The outlines of the overall design (see `qptr` below) have been around for a while,
but sadly lacking any direct attempts at "`alloc`/`#[global_allocator]` in a shader".

This experiment serves as both a challenge and a demonstration, of what is possible
in the short-to-medium term, within the confines of existing standards (SPIR-V) and
leveraging the existing Rust-GPU/SPIR-T infrastructure.

## How?

A combination of Rust-GPU/SPIR-T enhancements in various stages of development:
- some landed upstream, but still opt-in (e.g. `--no-infer-storage-classes`, `--spirt-passes=qptr`)
- others largely complete, but landing stalled on minor blockers (e.g. SPIR-T PRs [#50](https://github.com/EmbarkStudios/spirt/pull/50), [#52](https://github.com/EmbarkStudios/spirt/pull/52), [#29](https://github.com/EmbarkStudios/spirt/pull/29), [#42](https://github.com/EmbarkStudios/spirt/pull/42))
- the rest are brand new (from `alloc`-specific support, to general improvements
  in pointer flexibility, control-flow simplification, and their intersection etc.)

SPIR-T's `qptr` (as described [in its original PR](https://github.com/EmbarkStudios/spirt/pull/24)
and also [the Rust-GPU 0.7 release notes](https://github.com/EmbarkStudios/rust-gpu/releases/tag/v0.7.0))
plays an essential role, allowing low-level compiler transformations to be done
on untyped pointers, and only attempting to recover SPIR-V's much stricter
"typed logical pointers" _after_ legalizing away as much as possible.

As seen in `src/main.rs`, `RUSTGPU_CODEGEN_ARGS` is still required to opt into:
- `--no-early-report-zombies` to hide overzealous errors that can be legalized away
- `--no-infer-storage-classes` to skip the overcomplicated whole-program typed
  inference engine for SPIR-V address spaces ("storage classes"), in favor of
  `qptr`'s straight-forward post-legalization deductions
- `--no-legacy-mem2reg` to skip the less efficient "local variables -> SSA"
  transformation done on SPIR-V (in favor of its `qptr` replacement)
- `--spirt-passes=qptr,reduce,fuse_selects` to enable the `qptr` transformations
  (from/to typed pointers, plus the `mem2reg` replacement), while `reduce`+`fuse_selects`
  help clean up the implementation details of "zero-cost abstractions"

You can see some of those transformations in action in the `.spirt.html` file(s)
produced when e.g. `RUSTGPU_CODEGEN_ARGS="--dump-spirt-passes=$PWD"` is set.

The end result is `Box::new(123)` boiling down to:
- reserve an unique `idx` (e.g. by updating an `AtomicUsize` in a separate
  storage buffer, like the example `#[global_allocator]` does)
- `heap_storage_buffer[idx] = 123`

While more realistic examples would be noisier, it should always be possible to:
- express everything in terms of arrays and integers, if need be
  - some prior art in Mesa's `rusticl`+Zink, `clvk`+`clspv`, Vcc+Shady etc.
  - e.g. necessary for cross-compilation to GLSL/HLSL/WGSL
    (**TODO**: demo WGSL once Naga supports atomics in its SPIR-V front-end)
- choose a more direct representation of pointers, whenever one is available
  - e.g. `PhysicalStorageBuffer64` when targeting current Vulkan

## Where?

The exact `git` URLs and commit hashes for the Rust-GPU crates are tracked by
`Cargo.lock`, and _in theory_ `Cargo.toml`+`src/main.rs` could be repurposed
to get access to the same capabilities from some 3rd-party project.

_However_, specific commits are highly unlikely to be kept indefinitely
(especially the temporary merge commits), and the functionality itself may not
be suited for production use, only further experimentation production.
