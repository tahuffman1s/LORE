//! The two templates a game's shader is put into, and the building of a
//! module from one: no graphics card needed.

use std::sync::OnceLock;

use lore_shader::{
    Bounds, Compiled, Hook, Owner, Param, ParamBlock, ShaderError, Source, compile, hooked_module,
    params_of,
};

/// The engine's side of a screen effect, and where errors say it is.
const SCREEN: &str = include_str!("../../shaders/screen.wgsl");
const SCREEN_FILE: &str = "crates/lore-render/shaders/screen.wgsl";
/// The engine's side of a surface shader, which goes after the mesh shader.
const SURFACE: &str = include_str!("../../shaders/surface.wgsl");
const SURFACE_FILE: &str = "crates/lore-render/shaders/surface.wgsl";
/// The engine's side of a fluid shader, which goes after the mesh shader.
const FLUID: &str = include_str!("../../shaders/fluid_hook.wgsl");
const FLUID_FILE: &str = "crates/lore-render/shaders/fluid_hook.wgsl";
const MESH: &str = include_str!("../../shaders/mesh.wgsl");
const MESH_FILE: &str = "crates/lore-render/shaders/mesh.wgsl";

/// What to say about a fault in LORE (the same words `lore-shader` uses).
const FAULT_HINT: &str = "This is a fault in LORE, not in your shader. \
    Please report it at https://github.com/tahuffman1s/LORE/issues with this whole message.";

/// Where a screen effect's values are bound in its module.
const SCREEN_VALUES: (u32, u32) = (1, 0);
/// Where a surface shader's values are bound in its module.
const SURFACE_VALUES: (u32, u32) = (3, 0);
/// Where a fluid shader's values are bound in its module: after the set
/// the poured fluid's own pictures are in.
const FLUID_VALUES: (u32, u32) = (4, 0);

/// The marker lines in `lit` (`mesh.wgsl`) and the single line each becomes
/// in a game's module. The replacements must stay on one line, so that every
/// later line keeps its number.
const MARKERS: [(&str, &str); 3] = [
    (
        "//!surface settled",
        "let lore_made = lore_surface(corner, mixing, painted, base, normal, roughness, metallic); \
         base_seen = lore_made.color; \
         normal = towards_or(lore_made.normal, normal); \
         roughness = clamp(lore_made.roughness, SMOOTHEST, 1.0); \
         metallic = clamp(lore_made.metallic, 0.0, 1.0);",
    ),
    (
        "//!surface snowed",
        "base_seen = mix(lore_made.color, snow, snowed);",
    ),
    ("//!surface glows", "glow = lore_made.emission;"),
];

/// The marker lines of a fluid shader, in `water.wgsl` (a pool's pixel)
/// and `fluid.wgsl` (a drop in flight, and one lying on a surface), and the
/// single line each becomes in a game's module.
const FLUID_MARKERS: [(&str, &str); 3] = [
    (
        "//!fluid pool",
        "normal = lore_fluid_pool(corner, wet, normal);",
    ),
    (
        "//!fluid drop",
        "let lore_made = lore_fluid_drop(fluid_looks[kind * 3u], fluid_looks[kind * 3u + 1u], \
         fluid_looks[kind * 3u + 2u], trace.normal, \
         view.eye.xyz + trace.direction * max(trace.enters, 0.0), chord); \
         absorbs = lore_made.absorption; scatters = lore_made.scattering;",
    ),
    (
        "//!fluid film",
        "let lore_made = lore_fluid_drop(takes, gives, glows, up, \
         view.eye.xyz + direction * along, corner.third.y); \
         takes = vec4<f32>(lore_made.absorption, lore_made.roughness); \
         gives = vec4<f32>(lore_made.scattering, lore_made.metallic); \
         glows = vec4<f32>(lore_made.emission, glows.w);",
    ),
];

/// The files `mesh.wgsl` includes, as `build.rs` finds them on disk.
fn included(name: &str) -> Result<String, String> {
    let text = match name {
        "water.wgsl" => include_str!("../../shaders/water.wgsl"),
        "particles.wgsl" => include_str!("../../shaders/particles.wgsl"),
        "weather.wgsl" => include_str!("../../shaders/weather.wgsl"),
        "fluid.wgsl" => include_str!("../../shaders/fluid.wgsl"),
        "lamps.wgsl" => include_str!("../../shaders/lamps.wgsl"),
        "bounce.wgsl" => include_str!("../../shaders/bounce.wgsl"),
        "air.wgsl" => include_str!("../../shaders/air.wgsl"),
        "occlusion.wgsl" => include_str!("../../shaders/occlusion.wgsl"),
        "contact.wgsl" => include_str!("../../shaders/contact.wgsl"),
        "skylight.wgsl" => include_str!("../../shaders/skylight.wgsl"),
        other => return Err(format!("{other} is not one of the files built into LORE")),
    };
    Ok(text.to_owned())
}

/// A fault in LORE's own text, as a `ShaderError`.
pub(super) fn fault(file: &str, message: String) -> ShaderError {
    ShaderError {
        file: file.to_owned(),
        line: None,
        column: None,
        message,
        source_line: None,
        hint: Some(FAULT_HINT.to_owned()),
        note: None,
        lore_fault: true,
    }
}

/// `mesh` with each marker line swapped for its call. Each marker must be
/// there exactly once, or the mesh shader and this file have drifted apart.
fn with_markers_swapped(mesh: &str) -> Result<String, ShaderError> {
    for (marker, _) in MARKERS {
        let found = mesh.lines().filter(|line| line.trim() == marker).count();
        if found != 1 {
            return Err(fault(
                MESH_FILE,
                format!("The line `{marker}` should be there once and is there {found} times."),
            ));
        }
    }
    Ok(swapped(mesh, &MARKERS))
}

/// `text` with each line that is one of `markers` swapped for its call,
/// keeping the line's indent and every line's number.
fn swapped(text: &str, markers: &[(&str, &str)]) -> String {
    let mesh = text;
    let mut out = String::with_capacity(mesh.len() + 512);
    for line in mesh.lines() {
        match markers.iter().find(|(marker, _)| line.trim() == *marker) {
            Some((_, call)) => {
                let indent = &line[..line.len() - line.trim_start().len()];
                out.push_str(indent);
                out.push_str(call);
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// The engine's side of a fluid shader: the mesh shader with the fluid's
/// marker lines, in the files it includes, swapped for their calls. Each
/// marker must be there exactly once across those files.
fn fluid_engine() -> Result<Source, ShaderError> {
    let mut found = [0_usize; FLUID_MARKERS.len()];
    let with_calls = |name: &str| {
        let text = included(name)?;
        Ok(swapped(&text, &FLUID_MARKERS))
    };
    for name in ["water.wgsl", "fluid.wgsl"] {
        let text = included(name).map_err(|message| fault(MESH_FILE, message))?;
        for (count, (marker, _)) in found.iter_mut().zip(FLUID_MARKERS) {
            *count += text.lines().filter(|line| line.trim() == marker).count();
        }
    }
    for (count, (marker, _)) in found.iter().zip(FLUID_MARKERS) {
        if *count != 1 {
            return Err(fault(
                MESH_FILE,
                format!("The line `{marker}` should be there once and is there {count} times."),
            ));
        }
    }
    let mut source = Source::new();
    source
        .push_with_includes(Owner::Lore, MESH_FILE, MESH, with_calls)
        .map_err(|mut error| {
            error.lore_fault = true;
            error.hint = Some(FAULT_HINT.to_owned());
            error
        })?;
    source.push(Owner::Lore, FLUID_FILE, 1, FLUID);
    Ok(source)
}

/// The engine's side of a surface shader, built from the given mesh text.
fn surface_engine_from(mesh: &str) -> Result<Source, ShaderError> {
    let swapped = with_markers_swapped(mesh)?;
    let mut source = Source::new();
    source
        .push_with_includes(Owner::Lore, MESH_FILE, &swapped, included)
        .map_err(|mut error| {
            error.lore_fault = true;
            error.hint = Some(FAULT_HINT.to_owned());
            error
        })?;
    source.push(Owner::Lore, SURFACE_FILE, 1, SURFACE);
    Ok(source)
}

/// The engine's side of a hook, built once for the whole process.
fn engine(hook: Hook) -> Result<&'static Source, ShaderError> {
    static SCREEN_ENGINE: OnceLock<Result<Source, ShaderError>> = OnceLock::new();
    static SURFACE_ENGINE: OnceLock<Result<Source, ShaderError>> = OnceLock::new();
    static FLUID_ENGINE: OnceLock<Result<Source, ShaderError>> = OnceLock::new();
    let built = match hook {
        Hook::Screen => SCREEN_ENGINE.get_or_init(|| {
            let mut source = Source::new();
            source.push(Owner::Lore, SCREEN_FILE, 1, SCREEN);
            Ok(source)
        }),
        Hook::Surface => SURFACE_ENGINE.get_or_init(|| surface_engine_from(MESH)),
        Hook::Fluid => FLUID_ENGINE.get_or_init(fluid_engine),
    };
    built.as_ref().map_err(Clone::clone)
}

/// Builds the module a game's shader is drawn as, and compiles it with every
/// index kept inside what it indexes. Gives the compiled module and the block
/// that carries the shader's values.
pub(crate) fn build_module(
    hook: Hook,
    file: &str,
    text: &str,
) -> Result<(Compiled, ParamBlock), ShaderError> {
    let block = ParamBlock::new(params_of(file, text)?);
    let (group, binding) = match hook {
        Hook::Screen => SCREEN_VALUES,
        Hook::Surface => SURFACE_VALUES,
        Hook::Fluid => FLUID_VALUES,
    };
    let module = hooked_module(engine(hook)?, hook, &block, group, binding, file, text)?;
    let compiled = compile(&module, Bounds::Clamped)?;
    Ok((compiled, block))
}

/// Compiles a game's shader as it would be drawn, with no graphics card, and
/// gives its values: for `lore check`. `file` is the name errors show.
pub fn check_game_shader(hook: Hook, file: &str, text: &str) -> Result<Vec<Param>, ShaderError> {
    let (_, block) = build_module(hook, file, text)?;
    Ok(block.params().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lore_shader::ParamKind;

    #[test]
    fn a_file_with_both_functions_says_to_split_them() {
        let both = "fn screen(in: ScreenIn) -> vec3f { return in.color; }\n\
                    fn surface(in: SurfaceIn) -> Surface { return in.surface; }\n\
                    fn fluid(in: FluidIn) -> Fluid { return in.fluid; }\n";
        for hook in Hook::ALL {
            let error = check_game_shader(hook, "shaders/both.wgsl", both).unwrap_err();
            assert!(error.message.contains("both"), "{hook:?}: {error}");
            assert!(
                error
                    .hint
                    .as_deref()
                    .is_some_and(|h| h.contains("holds one function")),
                "{hook:?}: {error}"
            );
        }
    }

    #[test]
    fn each_hooks_starter_compiles() {
        for hook in Hook::ALL {
            let result = check_game_shader(hook, "shaders/starter.wgsl", hook.starter());
            assert_eq!(result, Ok(Vec::new()), "{hook:?}");
        }
    }

    #[test]
    fn a_screen_shader_gets_its_values() {
        let text = "//! param strength: f32 = 0.5 range 0..1\n\
                    //! param tint: color = #ff8800\n\
                    fn screen(in: ScreenIn) -> vec3f {\n\
                    \x20   let grey = dot(in.color, vec3f(0.2126, 0.7152, 0.0722));\n\
                    \x20   return mix(in.color, grey * params.tint, params.strength);\n\
                    }\n";
        let params = check_game_shader(Hook::Screen, "shaders/heat.wgsl", text).unwrap();
        let names: Vec<_> = params.iter().map(|p| (p.name.as_str(), p.kind)).collect();
        assert_eq!(
            names,
            [("strength", ParamKind::F32), ("tint", ParamKind::Color)]
        );
    }

    #[test]
    fn a_slip_is_reported_in_the_makers_file_and_line() {
        let text = "// a comment\n\nfn screen(in: ScreenIn) -> vec3f {\n    return in.colour;\n}\n";
        let error = check_game_shader(Hook::Screen, "shaders/slip.wgsl", text).unwrap_err();
        assert_eq!(error.file, "shaders/slip.wgsl");
        assert_eq!(error.line, Some(4));
        assert!(!error.lore_fault, "{error}");
    }

    #[test]
    fn a_slip_in_a_surface_shader_is_reported_in_the_makers_line() {
        let text = "fn surface(in: SurfaceIn) -> Surface {\n    var made = in.surface;\n    made.colour = vec3f(1.0);\n    return made;\n}\n";
        let error = check_game_shader(Hook::Surface, "shaders/lava.wgsl", text).unwrap_err();
        assert_eq!(error.file, "shaders/lava.wgsl");
        assert_eq!(error.line, Some(3));
        assert!(!error.lore_fault, "{error}");
    }

    #[test]
    fn a_missing_marker_is_a_fault_in_lore() {
        let (marker, _) = MARKERS[1];
        let without: String = MESH
            .lines()
            .filter(|line| line.trim() != marker)
            .map(|line| format!("{line}\n"))
            .collect();
        let error = surface_engine_from(&without).unwrap_err();
        assert!(error.lore_fault, "{error}");
        assert_eq!(error.file, MESH_FILE);
        assert!(error.message.contains(marker), "{error}");
    }

    #[test]
    fn a_marker_twice_is_a_fault_in_lore() {
        let (marker, _) = MARKERS[2];
        let twice = format!("{MESH}\n{marker}\n");
        let error = surface_engine_from(&twice).unwrap_err();
        assert!(error.lore_fault, "{error}");
    }

    #[test]
    fn swapping_markers_keeps_every_line_number() {
        let swapped = with_markers_swapped(MESH).unwrap();
        assert_eq!(swapped.lines().count(), MESH.lines().count());
        assert!(
            swapped
                .lines()
                .all(|line| !line.trim().starts_with("//!surface "))
        );
        assert!(swapped.contains("let lore_made = lore_surface("));
    }

    #[test]
    fn a_screen_shader_can_call_distance_at_and_picture() {
        let text = "fn screen(in: ScreenIn) -> vec3f {\n\
                    \x20   let near = distance_at(in.uv + vec2f(0.01, 0.0));\n\
                    \x20   let blurred = picture(in.uv + vec2f(0.0, 0.01));\n\
                    \x20   return mix(in.color, blurred, clamp(near / 100.0, 0.0, 1.0)) * (1.0 + in.time * 0.0)\n\
                    \x20       + vec3f(in.size.x * 0.0 + in.distance * 0.0);\n\
                    }\n";
        check_game_shader(Hook::Screen, "shaders/calls.wgsl", text).unwrap();
    }

    #[test]
    fn a_fluid_shader_can_use_every_field_and_the_helpers() {
        let text = "//! param speed: f32 = 0.2 range 0..2\n\
                    fn fluid(in: FluidIn) -> Fluid {\n\
                    \x20   var made = in.fluid;\n\
                    \x20   let wave = sin(in.time * params.speed + in.position.x + in.distance * 0.01);\n\
                    \x20   made.absorption = in.fluid.absorption * (1.0 + in.depth);\n\
                    \x20   made.scattering = in.fluid.scattering + fluid_color(in.fluid) * in.pool;\n\
                    \x20   made.emission = vec3f(wave * wave);\n\
                    \x20   made.roughness = in.fluid.roughness;\n\
                    \x20   made.metallic = in.fluid.metallic;\n\
                    \x20   made.normal = normalize(in.fluid.normal + in.view * 0.01);\n\
                    \x20   return fluid_colored(made, vec3f(0.9, 0.3, 0.05));\n\
                    }\n";
        let params = check_game_shader(Hook::Fluid, "shaders/every.wgsl", text).unwrap();
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn a_slip_in_a_fluid_shader_is_reported_in_the_makers_line() {
        let text = "fn fluid(in: FluidIn) -> Fluid {\n    var made = in.fluid;\n    made.glow = vec3f(1.0);\n    return made;\n}\n";
        let error = check_game_shader(Hook::Fluid, "shaders/crust.wgsl", text).unwrap_err();
        assert_eq!(error.file, "shaders/crust.wgsl");
        assert_eq!(error.line, Some(3));
        assert!(!error.lore_fault, "{error}");
    }

    #[test]
    fn a_fluid_shaders_markers_are_each_there_once_and_swapped_on_their_own_lines() {
        let water = swapped(&included("water.wgsl").unwrap(), &FLUID_MARKERS);
        let fluid = swapped(&included("fluid.wgsl").unwrap(), &FLUID_MARKERS);
        assert_eq!(
            water.lines().count(),
            included("water.wgsl").unwrap().lines().count()
        );
        assert!(water.contains("normal = lore_fluid_pool(corner, wet, normal);"));
        assert!(fluid.contains("absorbs = lore_made.absorption;"));
        assert!(fluid.contains("glows = vec4<f32>(lore_made.emission, glows.w);"));
        for text in [&water, &fluid] {
            assert!(
                text.lines()
                    .all(|line| !line.trim().starts_with("//!fluid "))
            );
        }
        // A surface shader's module leaves them as the comments they are.
        let surface = surface_engine_from(MESH).unwrap();
        assert!(surface.text().contains("//!fluid pool"));
    }

    #[test]
    fn a_surface_shader_can_use_every_field() {
        let text = "fn surface(in: SurfaceIn) -> Surface {\n\
                    \x20   var made: Surface;\n\
                    \x20   let pulse = 0.5 + 0.5 * sin(in.time + in.position.x + in.uv.y + in.distance * 0.01);\n\
                    \x20   made.color = in.surface.color * pulse;\n\
                    \x20   made.normal = normalize(in.surface.normal + in.view * 0.01);\n\
                    \x20   made.roughness = in.surface.roughness;\n\
                    \x20   made.metallic = in.surface.metallic;\n\
                    \x20   made.emission = in.surface.emission + vec3f(pulse);\n\
                    \x20   made.alpha = in.surface.alpha;\n\
                    \x20   return made;\n\
                    }\n";
        check_game_shader(Hook::Surface, "shaders/every.wgsl", text).unwrap();
    }
}
