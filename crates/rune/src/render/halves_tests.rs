use super::*;
use wgpu::naga;

/// What Naga's validator says of `wgsl` on a device with `capabilities`.
fn validates(wgsl: &str, capabilities: naga::valid::Capabilities) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(wgsl).map_err(|e| e.emit_to_string(wgsl))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), capabilities)
        .validate(&module)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// The capabilities of a device without `SHADER_F16_IN_F32`, the emulator's guest Vulkan.
fn without_halves() -> naga::valid::Capabilities {
    naga::valid::Capabilities::all() - naga::valid::Capabilities::SHADER_FLOAT16_IN_FLOAT32
}

#[test]
fn a_device_without_halves_refuses_the_uber_shader_and_accepts_its_portable_form() {
    let uber = crate::render::metallib::uber_wgsl();
    assert!(validates(&uber, without_halves()).is_err(), "the uber shader must need the capability, or this test proves nothing");
    let portable = portable(&uber).expect("the uber shader unpacks halves");
    validates(&portable, without_halves()).expect("the portable uber shader");
    assert!(!portable.replace("rune_unpack2x16float(", "").contains("unpack2x16float("));
}

#[test]
fn every_shader_that_unpacks_halves_has_a_portable_form() {
    let sources = [
        ("backdrop composite", format!("{}\n{}", crate::render::CLIP_WGSL, include_str!("../backdrop_composite.wgsl"))),
        ("video", include_str!("../native_surface/video.wgsl").to_owned()),
    ];
    for (name, wgsl) in sources {
        let portable = portable(&wgsl).unwrap_or_else(|| panic!("{name} unpacks halves"));
        validates(&portable, without_halves()).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn a_shader_that_unpacks_no_halves_is_left_alone() {
    assert_eq!(portable("@fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0); }"), None);
}

/// A half's value by arithmetic, independent of the decoder's bit moves.
fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = i32::from((bits >> 10) & 0x1f);
    let m = f32::from(bits & 0x3ff);
    match e {
        0 => sign * m * 2f32.powi(-24),
        31 if m == 0.0 => sign * f32::INFINITY,
        31 => f32::NAN,
        _ => sign * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

#[test]
fn the_portable_decoder_gives_every_half_its_value() {
    let gpu = crate::render::Gpu::headless_wgpu();
    let (device, queue) = (&gpu.device, &gpu.queue);
    let source = format!(
        "{DECODER}\n@group(0) @binding(0) var<storage, read_write> out: array<vec2<f32>>;\n\
         @compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{\n\
             let i = id.x;\n\
             if i < 32768u {{ out[i] = rune_unpack2x16float((i * 2u) | ((i * 2u + 1u) << 16u)); }}\n\
         }}"
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("halves"), source: wgpu::ShaderSource::Wgsl(source.into()) });
    let size = 65536 * 4;
    let out = device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
    let read = device.create_buffer(&wgpu::BufferDescriptor { label: None, size, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None, layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: out.as_entire_binding() }],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(32768 / 64, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out, 0, &read, 0, size);
    queue.submit([encoder.finish()]);
    read.slice(..).map_async(wgpu::MapMode::Read, |r| r.expect("map"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    let values: Vec<f32> = bytemuck::cast_slice(&read.slice(..).get_mapped_range().expect("mapped")).to_vec();
    for (bits, got) in values.iter().enumerate() {
        let want = half(bits as u16);
        if want.is_nan() {
            assert!(got.is_nan(), "{bits:#06x}: {got} is not NaN");
        } else {
            assert_eq!(got.to_bits(), want.to_bits(), "{bits:#06x}: {got} != {want}");
        }
    }
}
