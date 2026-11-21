//! Half floats unpacked without the GPU's own instruction, for a device that refuses it.
//!
//! WGSL's `unpack2x16float` needs the adapter's `SHADER_F16_IN_F32` downlevel flag. Vulkan
//! drivers that lack it exist: llvmpipe without native `f16`, and the Android emulator's guest
//! Vulkan, whose refusal of the uber shader's `clip_alpha` aborted a TV app at its first frame.
//! [`shader_module`] compiles a shader as written and, when the device refuses it and the source
//! unpacks halves, compiles it again with [`portable`]'s bit-level decoder instead: the same
//! values for every finite half, and the device keeps drawing.

/// The decoder `portable` puts in place of `unpack2x16float`. Component 0 is the low 16 bits,
/// as WGSL's own. Normals and infinities move their bits; subnormals scale by 2^-24.
const DECODER: &str = r"
fn rune_half_to_f32(h: u32) -> f32 {
    let sign = (h & 0x8000u) << 16u;
    let e = (h >> 10u) & 0x1fu;
    let m = h & 0x3ffu;
    if e == 0u {
        let v = f32(m) * 5.9604644775390625e-8;
        return select(v, -v, sign != 0u);
    }
    let exp = select(e + 112u, 255u, e == 31u);
    return bitcast<f32>(sign | (exp << 23u) | (m << 13u));
}

fn rune_unpack2x16float(w: u32) -> vec2<f32> {
    return vec2<f32>(rune_half_to_f32(w & 0xffffu), rune_half_to_f32(w >> 16u));
}
";

/// `wgsl` with every `unpack2x16float` call made through [`DECODER`], or `None` when it unpacks
/// no halves and so has nothing to replace.
pub fn portable(wgsl: &str) -> Option<String> {
    if !wgsl.contains("unpack2x16float(") {
        return None;
    }
    Some(format!("{DECODER}\n{}", wgsl.replace("unpack2x16float(", "rune_unpack2x16float(")))
}

/// `device.create_shader_module` for `wgsl`, falling back to [`portable`]'s source when the
/// device refuses the original and the portable one is accepted. A source the device refuses
/// either way is compiled as written, unscoped, so the refusal reaches the device's error
/// handler as it always has.
pub fn shader_module(device: &wgpu::Device, label: &str, wgsl: &str) -> wgpu::ShaderModule {
    let compile = |source: &str| device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(portable) = portable(wgsl) {
        let (module, refused) = scoped(device, || compile(wgsl));
        let Some(refused) = refused else { return module };
        let (module, still) = scoped(device, || compile(&portable));
        if still.is_none() {
            eprintln!("rune: {label}: the device refused unpack2x16float ({refused}); unpacking halves in the shader");
            return module;
        }
    }
    compile(wgsl)
}

/// `make` inside validation and internal error scopes, with the first error either caught.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn scoped<T>(device: &wgpu::Device, make: impl FnOnce() -> T) -> (T, Option<wgpu::Error>) {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let made = make();
    let refused = pollster::block_on(internal.pop()).or(pollster::block_on(validation.pop()));
    (made, refused)
}

#[cfg(test)]
#[path = "halves_tests.rs"]
mod tests;
