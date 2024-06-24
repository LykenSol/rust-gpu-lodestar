// FIXME(eddyb) this was meant to be `-Z script` but that's unergonomic.
// #!/usr/bin/env -S cargo run -Zscript --release --manifest-path

fn main() {
    std::env::set_var(
        "RUSTGPU_CODEGEN_ARGS",
        [
            "--no-early-report-zombies --no-infer-storage-classes --no-legacy-mem2reg \
             --spirt-passes=qptr,reduce,fuse_selects",
            &std::env::var("RUSTGPU_CODEGEN_ARGS").unwrap_or_default(),
        ]
        .join(" "),
    );
    let args = std::env::args_os();
    if args.len() == 1 {
        eprintln!("Usage: cargo run --release [SHADER DIR]");
        eprintln!("  (e.g: `cargo run --release examples/working`)");
        std::process::exit(1);
    }
    let mut any_errors = false;
    for path in args.skip(1) {
        let result = spirv_builder::SpirvBuilder::new(path, "spirv-unknown-vulkan1.2")
            .capability(spirv_builder::Capability::Int8)
            .capability(spirv_builder::Capability::VulkanMemoryModelDeviceScopeKHR)
            .print_metadata(spirv_builder::MetadataPrintout::None)
            .build();

        if let Ok(result) = result {
            let spv_bytes = std::fs::read(result.module.unwrap_single()).unwrap();
            let spv_words = wgpu::util::make_spirv_raw(&spv_bytes);

            for entry_point in &result.entry_points {
                futures::executor::block_on(run_async(&spv_words, entry_point));
            }
        } else {
            any_errors = true;
        }
    }
    if any_errors {
        std::process::exit(1);
    }
}

async fn run_async(spv_words: &[u32], entry_point: &str) {
    use wgpu::util::DeviceExt as _;

    // FIXME(eddyb) get rid of this when `naga` supports atomics.
    let force_spirv_passthru = true;

    let backends = wgpu::util::backend_bits_from_env().unwrap_or(wgpu::Backends::PRIMARY);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        dx12_shader_compiler: wgpu::util::dx12_shader_compiler_from_env().unwrap_or_default(),
        ..Default::default()
    });
    let adapter = wgpu::util::initialize_adapter_from_env_or_default(&instance, None)
        .await
        .expect("Failed to find an appropriate adapter");

    let mut required_features =
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    if force_spirv_passthru {
        required_features |= wgpu::Features::SPIRV_SHADER_PASSTHROUGH;
    }

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features,
                required_limits: wgpu::Limits::default(),
            },
            None,
        )
        .await
        .expect("Failed to create device");
    drop(instance);
    drop(adapter);

    let timestamp_period = queue.get_timestamp_period();

    // FIXME(eddyb) automate this decision by default.
    let module = if force_spirv_passthru {
        unsafe {
            device.create_shader_module_spirv(&wgpu::ShaderModuleDescriptorSpirV {
                label: None,
                source: spv_words.into(),
            })
        }
    } else {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::SpirV(spv_words.into()),
        })
    };

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                count: None,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    has_dynamic_offset: false,
                    min_binding_size: None,
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                },
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                count: None,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    has_dynamic_offset: false,
                    min_binding_size: None,
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                },
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point,
        compilation_options: Default::default(),
    });

    let heap_size = 32 * 1024;
    let heap_unit_size = 4;

    let readback_buffer_heap = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: heap_size as wgpu::BufferAddress,
        // Can be read to the CPU, and can be copied from the shader's storage buffer
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let readback_buffer_remaining_atomic = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4,
        // Can be read to the CPU, and can be copied from the shader's storage buffer
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let storage_buffer_heap = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Heap memory area"),
        size: heap_size as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let storage_buffer_remaining_atomic =
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Remaining atomic for heap"),
            contents: &u32::to_ne_bytes(heap_size / heap_unit_size),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });

    let timestamp_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Timestamps buffer"),
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let timestamp_readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: true,
    });
    timestamp_readback_buffer.unmap();

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: storage_buffer_heap.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: storage_buffer_remaining_atomic.as_entire_binding(),
            },
        ],
    });

    let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        count: 2,
        ty: wgpu::QueryType::Timestamp,
    });

    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

    {
        let mut cpass = encoder.begin_compute_pass(&Default::default());
        cpass.set_bind_group(0, &bind_group, &[]);
        cpass.set_pipeline(&compute_pipeline);
        cpass.write_timestamp(&queries, 0);
        cpass.dispatch_workgroups(1, 1, 1);
        cpass.write_timestamp(&queries, 1);
    }

    encoder.copy_buffer_to_buffer(
        &storage_buffer_heap,
        0,
        &readback_buffer_heap,
        0,
        heap_size as wgpu::BufferAddress,
    );
    encoder.copy_buffer_to_buffer(
        &storage_buffer_remaining_atomic,
        0,
        &readback_buffer_remaining_atomic,
        0,
        4,
    );
    encoder.resolve_query_set(&queries, 0..2, &timestamp_buffer, 0);
    encoder.copy_buffer_to_buffer(
        &timestamp_buffer,
        0,
        &timestamp_readback_buffer,
        0,
        timestamp_buffer.size(),
    );

    queue.submit(Some(encoder.finish()));
    let buffer_heap_slice = readback_buffer_heap.slice(..);
    let buffer_remaining_atomic_slice = readback_buffer_remaining_atomic.slice(..);
    let timestamp_slice = timestamp_readback_buffer.slice(..);
    timestamp_slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
    buffer_heap_slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
    buffer_remaining_atomic_slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
    // NOTE(eddyb) `poll` should return only after the above callbacks fire
    // (see also https://github.com/gfx-rs/wgpu/pull/2698 for more details).
    device.poll(wgpu::Maintain::Wait);

    let heap_data = buffer_heap_slice.get_mapped_range();
    let remaining_atomic_data = buffer_remaining_atomic_slice.get_mapped_range();
    let timing_data = timestamp_slice.get_mapped_range();
    let remaining_atomic_contents =
        u32::from_ne_bytes((&remaining_atomic_data[..]).try_into().unwrap());
    let heap_contents = heap_data
        .chunks_exact(heap_unit_size as usize)
        .skip(remaining_atomic_contents as usize)
        .map(|b| u32::from_ne_bytes(b.try_into().unwrap()))
        .collect::<Vec<_>>();
    let timings = timing_data
        .chunks_exact(8)
        .map(|b| u64::from_ne_bytes(b.try_into().unwrap()))
        .collect::<Vec<_>>();
    drop(heap_data);
    readback_buffer_heap.unmap();
    drop(remaining_atomic_data);
    readback_buffer_remaining_atomic.unmap();
    drop(timing_data);
    timestamp_readback_buffer.unmap();

    println!(
        "{entry_point}: allocated {} bytes in {:?}, leaving this heap behind:",
        heap_size - remaining_atomic_contents * heap_unit_size,
        std::time::Duration::from_nanos(
            ((timings[1] - timings[0]) as f64 * f64::from(timestamp_period)) as u64
        )
    );
    for chunk in heap_contents.chunks(8) {
        print!(" ");
        for &word in chunk {
            print!(" {word:08x}");
        }
        println!();
    }
}
