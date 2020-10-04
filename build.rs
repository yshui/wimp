use ::anyhow::Result;
use ::shaderc::{Compiler, ShaderKind};
use ::std::path::Path;
fn compile_shader(file: impl AsRef<Path>, compiler: &mut Compiler) -> Result<()> {
    let source = ::std::fs::read_to_string(Path::new("shaders").join(file.as_ref()))?;
    let kind = match file.as_ref().extension().and_then(|x| x.to_str()) {
        Some("vert") => ShaderKind::Vertex,
        Some("frag") => ShaderKind::Fragment,
        Some(ext) => panic!("Invalid shader file extension {}", ext),
        None => panic!("Shader file needs an extension"),
    };
    let output = compiler.compile_into_spirv(
        &source,
        kind,
        file.as_ref().to_string_lossy().into_owned().as_str(),
        "main",
        None,
    )?;
    let output_name = match kind {
        ShaderKind::Vertex => "vert.spv",
        ShaderKind::Fragment => "frag.spv",
        _ => panic!("Unhandled shader kind"),
    };
    let out_dir = ::std::env::var("OUT_DIR")?;
    let out_dir = Path::new(out_dir.as_str());
    ::std::fs::write(out_dir.join(output_name), output.as_binary_u8())?;
    Ok(())
}

fn main() {
    let mut compiler = Compiler::new().unwrap();
    compile_shader("shader.vert", &mut compiler).unwrap();
    compile_shader("shader.frag", &mut compiler).unwrap();
}
