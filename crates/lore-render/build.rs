//! Compiles the WGSL shaders in `shaders/` to SPIR-V with naga.
//!
//! Doing this in pure Rust means building LORE needs no Vulkan SDK or
//! system shader compiler. The stitching of `//!include` lines and the
//! compiling are `lore-shader`'s, the same code that checks a game's shaders.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::{env, fs};

use lore_shader::{Bounds, Owner, Source, compile};

/// Shader files to compile, by name without the `.wgsl` extension.
const SHADERS: &[&str] = &["eye", "grid", "mesh", "ripples", "tonemap", "ui", "waves"];

fn main() -> Result<(), Box<dyn Error>> {
    let out = PathBuf::from(env::var("OUT_DIR")?);
    for name in SHADERS {
        let words = build(name)?;
        let bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        fs::write(out.join(format!("{name}.spv")), bytes)?;
    }
    Ok(())
}

/// Turns one WGSL file into SPIR-V holding all of its entry points.
fn build(name: &str) -> Result<Vec<u32>, Box<dyn Error>> {
    let path = Path::new("shaders").join(format!("{name}.wgsl"));
    println!("cargo::rerun-if-changed={}", path.display());
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let mut source = Source::new();
    source.push_with_includes(Owner::Author, &path.to_string_lossy(), &text, |included| {
        let included = Path::new("shaders").join(included);
        println!("cargo::rerun-if-changed={}", included.display());
        fs::read_to_string(&included).map_err(|error| format!("{}: {error}", included.display()))
    })?;
    // The engine's own shaders are trusted: no index guards, as before.
    Ok(compile(&source, Bounds::Trusted)?.words)
}
