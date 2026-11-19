//! A game's shader and the engine's template around it.

use crate::scan::{Token, blank_comments, error_at, refuse_endless_loops, tokens};
use crate::{MAX_TEXT_BYTES, Owner, ParamBlock, ShaderError, Source};

/// What a game shader is used for, which decides the function it must hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hook {
    /// An effect over the finished picture: `fn screen(in: ScreenIn) -> vec3f`.
    Screen,
    /// A change to the surface of a material: `fn surface(in: SurfaceIn) -> Surface`.
    Surface,
}

impl Hook {
    /// The name of the function the shader must hold: `screen` or `surface`.
    pub fn entry(self) -> &'static str {
        match self {
            Self::Screen => "screen",
            Self::Surface => "surface",
        }
    }

    /// The first line of that function, with no `{`.
    pub fn signature(self) -> &'static str {
        match self {
            Self::Screen => "fn screen(in: ScreenIn) -> vec3f",
            Self::Surface => "fn surface(in: SurfaceIn) -> Surface",
        }
    }

    /// The smallest whole function: the signature, a `return` that changes
    /// nothing, and `}`. These are the three lines a missing-function error shows.
    pub fn starter(self) -> &'static str {
        match self {
            Self::Screen => "fn screen(in: ScreenIn) -> vec3f {\n    return in.color;\n}",
            Self::Surface => "fn surface(in: SurfaceIn) -> Surface {\n    return in.surface;\n}",
        }
    }

    /// The type the function takes.
    fn input(self) -> &'static str {
        match self {
            Self::Screen => "ScreenIn",
            Self::Surface => "SurfaceIn",
        }
    }

    /// What the function gives back, as the pieces a scan sees.
    fn output(self) -> &'static [&'static str] {
        match self {
            Self::Screen => &["vec3f"],
            Self::Surface => &["Surface"],
        }
    }

    /// The Luau call that uses a shader of this kind.
    fn call(self) -> &'static str {
        match self {
            Self::Screen => "game.shaders.screen",
            Self::Surface => "game.shaders.surface",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Screen => Self::Surface,
            Self::Surface => Self::Screen,
        }
    }

    /// What a shader of this kind is called in a sentence.
    fn kind(self) -> &'static str {
        match self {
            Self::Screen => "a screen effect",
            Self::Surface => "a surface shader",
        }
    }

    /// The starter, each line indented so it reads well under `Hint:`.
    fn starter_indented(self) -> String {
        self.starter()
            .lines()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The module a game's shader is built as: `engine` (the template, already
/// stitched), then `params.wgsl(group, binding)` as LORE's text, then `text`
/// as the maker's own file `file`.
///
/// Before naga sees anything it checks the size of `text`, refuses endless
/// loops, and checks that `text` holds the entry function the `hook` needs,
/// with the right shape. That is a scan of the text, so a wrong shape is
/// reported on the maker's own line and not at the template's call of it.
pub fn hooked_module(
    engine: &Source,
    hook: Hook,
    params: &ParamBlock,
    group: u32,
    binding: u32,
    file: &str,
    text: &str,
) -> Result<Source, ShaderError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(ShaderError::whole_file(
            file,
            format!(
                "This shader is {} KiB, and a game shader can hold at most {} KiB.",
                text.len().div_ceil(1024),
                MAX_TEXT_BYTES / 1024
            ),
            Some("Remove what you do not use, or split the work between two shaders.".to_owned()),
        ));
    }
    refuse_endless_loops(file, text)?;
    check_entry(hook, file, text)?;
    let mut module = engine.clone();
    module.push(
        Owner::Lore,
        "params (written by LORE from the //! param lines)",
        1,
        &params.wgsl(group, binding),
    );
    module.push(Owner::Author, file, 1, text);
    Ok(module)
}

/// Checks that `text` has `fn <entry>` with the signature `hook` needs.
fn check_entry(hook: Hook, file: &str, text: &str) -> Result<(), ShaderError> {
    let blanked = blank_comments(text);
    let pieces = tokens(&blanked);
    let Some(at) = find_fn(&pieces, hook.entry()) else {
        let other = hook.other();
        let hint = if find_fn(&pieces, other.entry()).is_some() {
            format!(
                "It has a `{}` function, so it is {}: use it with `{}`. To use it as {} \
                 instead, write a `{}` function like this:\n{}",
                other.entry(),
                other.kind(),
                other.call(),
                hook.kind(),
                hook.entry(),
                hook.starter_indented()
            )
        } else {
            format!("Add a function like this:\n{}", hook.starter_indented())
        };
        return Err(ShaderError::whole_file(
            file,
            format!(
                "This shader has no `{}` function, and {} needs one.",
                hook.entry(),
                hook.kind()
            ),
            Some(hint),
        ));
    };
    if !has_shape(&pieces, at + 2, hook) {
        return Err(error_at(
            file,
            text,
            pieces[at].at,
            format!(
                "`{}` must take one `{}` and give back {}.",
                hook.entry(),
                hook.input(),
                hook.output().join("")
            ),
            Some(format!(
                "Write its first line like this: `{} {{`",
                hook.signature()
            )),
        ));
    }
    Ok(())
}

/// The index of the `fn` that starts `fn <name>`, as whole words.
fn find_fn(pieces: &[Token<'_>], name: &str) -> Option<usize> {
    pieces
        .windows(2)
        .position(|pair| pair[0].text == "fn" && pair[1].text == name)
}

/// Whether the pieces from `from` (just after the function's name) are
/// `( x: Input [,] ) -> Output {`.
fn has_shape(pieces: &[Token<'_>], from: usize, hook: Hook) -> bool {
    let mut cursor = Cursor { pieces, next: from };
    if !(cursor.eat("(") && cursor.eat_word() && cursor.eat(":") && cursor.eat(hook.input())) {
        return false;
    }
    // A trailing comma is allowed.
    cursor.eat(",");
    if !(cursor.eat(")") && cursor.eat("->")) {
        return false;
    }
    let output: &[&str] = match hook {
        // `vec3f` and `vec3<f32>` are the same type.
        Hook::Screen
            if cursor
                .pieces
                .get(cursor.next)
                .is_some_and(|p| p.text == "vec3") =>
        {
            &["vec3", "<", "f32", ">"]
        }
        _ => hook.output(),
    };
    output.iter().all(|piece| cursor.eat(piece)) && cursor.eat("{")
}

/// A place in a list of pieces, moved on by what matches.
struct Cursor<'a, 'b> {
    pieces: &'a [Token<'b>],
    next: usize,
}

impl Cursor<'_, '_> {
    /// Moves past the next piece if it is `text`.
    fn eat(&mut self, text: &str) -> bool {
        let ok = self.pieces.get(self.next).is_some_and(|p| p.text == text);
        self.next += usize::from(ok);
        ok
    }

    /// Moves past the next piece if it is a word.
    fn eat_word(&mut self) -> bool {
        let ok = self.pieces.get(self.next).is_some_and(Token::is_word);
        self.next += usize::from(ok);
        ok
    }
}

#[cfg(test)]
mod tests {
    use crate::{Bounds, ParamBlock, compile, params_of};

    use super::*;

    const FILE: &str = "shaders/heat.wgsl";
    const TEMPLATE_FILE: &str = "engine/screen_template.wgsl";

    /// A stand-in for the engine's screen template.
    const SCREEN: &str =
        "struct ScreenIn { uv: vec2f, color: vec3f, distance: f32, time: f32, size: vec2f }
fn picture(uv: vec2f) -> vec3f { return vec3f(uv, 0.5); }
fn distance_at(uv: vec2f) -> f32 { return uv.x * 100.0; }
struct VOut { @builtin(position) position: vec4f, @location(0) uv: vec2f }
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VOut {
    let uv = vec2f(f32((index << 1u) & 2u), f32(index & 2u));
    return VOut(vec4f(uv * 2.0 - 1.0, 0.0, 1.0), uv);
}
@fragment
fn fs_main(v: VOut) -> @location(0) vec4f {
    var input: ScreenIn;
    input.uv = v.uv;
    input.color = picture(v.uv);
    input.distance = distance_at(v.uv);
    input.time = 1.0;
    input.size = vec2f(1280.0, 720.0);
    return vec4f(screen(input), 1.0);
}
";

    /// A stand-in for the engine's surface template.
    const SURFACE: &str = "struct Surface { color: vec3f, normal: vec3f, roughness: f32, metallic: f32, emission: vec3f, alpha: f32 }
struct SurfaceIn { surface: Surface, position: vec3f, uv: vec2f, view: vec3f, distance: f32, time: f32 }
@fragment
fn fs_main(@location(0) uv: vec2f) -> @location(0) vec4f {
    var base: Surface;
    base.color = vec3f(uv, 0.0);
    base.normal = vec3f(0.0, 1.0, 0.0);
    var input: SurfaceIn;
    input.surface = base;
    input.uv = uv;
    let made = surface(input);
    return vec4f(made.color + made.emission, made.alpha);
}
";

    fn template(text: &str) -> Source {
        let mut engine = Source::new();
        engine.push(Owner::Lore, TEMPLATE_FILE, 1, text);
        engine
    }

    fn build(hook: Hook, engine: &str, text: &str) -> Result<Source, ShaderError> {
        let params = ParamBlock::new(params_of(FILE, text)?);
        hooked_module(&template(engine), hook, &params, 1, 0, FILE, text)
    }

    fn screen_error(text: &str) -> ShaderError {
        build(Hook::Screen, SCREEN, text).unwrap_err()
    }

    const HEAT: &str = "//! param strength: f32 = 0.5 range 0..1
//! param tint: color = #ff8800
fn screen(in: ScreenIn) -> vec3f {
    let grey = dot(in.color, vec3f(0.2126, 0.7152, 0.0722));
    return mix(in.color, grey * params.tint, params.strength);
}
";

    #[test]
    fn a_screen_effect_with_values_compiles() {
        let module = build(Hook::Screen, SCREEN, HEAT).unwrap();
        assert!(
            module
                .text()
                .contains("@group(1) @binding(0) var<uniform> params")
        );
        let compiled = compile(&module, Bounds::Clamped).unwrap();
        assert_eq!(compiled.entry_points, ["vs_main", "fs_main"]);
        assert_eq!(compiled.words.first(), Some(&0x0723_0203));
    }

    #[test]
    fn a_surface_shader_compiles() {
        let text = "fn surface(in: SurfaceIn) -> Surface {\n    var s = in.surface;\n    s.emission = vec3f(1.0, 0.4, 0.0) * sin(in.time);\n    return s;\n}\n";
        let module = build(Hook::Surface, SURFACE, text).unwrap();
        let compiled = compile(&module, Bounds::Clamped).unwrap();
        assert_eq!(compiled.entry_points, ["fs_main"]);
    }

    #[test]
    fn a_slip_on_the_makers_line_is_reported_in_the_makers_file() {
        let text = "//! param strength: f32 = 0.5\nfn screen(in: ScreenIn) -> vec3f {\n    return in.colour * params.strength;\n}\n";
        let module = build(Hook::Screen, SCREEN, text).unwrap();
        let error = compile(&module, Bounds::Clamped).unwrap_err();
        eprintln!("{error}");
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(3));
        assert_eq!(error.column, Some(15));
        assert!(!error.lore_fault);
        assert_eq!(error.hint.as_deref(), Some("Did you mean `color`?"));
    }

    #[test]
    fn the_same_slip_in_a_surface_shader() {
        let text = "fn surface(in: SurfaceIn) -> Surface {\n    var s = in.surface;\n    s.colour = vec3f(1.0);\n    return s;\n}\n";
        let module = build(Hook::Surface, SURFACE, text).unwrap();
        let error = compile(&module, Bounds::Clamped).unwrap_err();
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(3));
        assert_eq!(error.column, Some(7));
        assert_eq!(error.hint.as_deref(), Some("Did you mean `color`?"));
    }

    #[test]
    fn a_misspelt_value_suggests_the_right_one() {
        let text = "//! param strength: f32 = 0.5\nfn screen(in: ScreenIn) -> vec3f {\n    return in.color * params.strenght;\n}\n";
        let module = build(Hook::Screen, SCREEN, text).unwrap();
        let error = compile(&module, Bounds::Clamped).unwrap_err();
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(3));
        assert!(error.hint.unwrap().contains("Did you mean `strength`?"));
    }

    #[test]
    fn a_slip_in_the_template_is_a_fault_in_lore() {
        let broken = SCREEN.replace("input.time = 1.0;", "input.timee = 1.0;");
        let module = build(Hook::Screen, &broken, HEAT).unwrap();
        let error = compile(&module, Bounds::Clamped).unwrap_err();
        assert!(error.lore_fault);
        assert_eq!(error.file, TEMPLATE_FILE);
        assert_eq!(error.line, Some(16));
        assert!(error.message.contains(FILE));
    }

    #[test]
    fn a_missing_function_shows_the_starter() {
        let error = screen_error("fn other() {}\n");
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, None);
        assert!(error.message.contains("no `screen` function"));
        let hint = error.hint.unwrap();
        assert!(hint.contains("    fn screen(in: ScreenIn) -> vec3f {\n    "));
        assert!(hint.contains("return in.color;"));
        let error = build(Hook::Surface, SURFACE, "fn other() {}").unwrap_err();
        assert!(error.message.contains("a surface shader needs one"));
        assert!(error.hint.unwrap().contains("return in.surface;"));
    }

    #[test]
    fn a_function_for_the_other_hook_is_pointed_out() {
        let text = "fn surface(in: SurfaceIn) -> Surface { return in.surface; }";
        let hint = screen_error(text).hint.unwrap();
        assert!(hint.starts_with("It has a `surface` function, so it is a surface shader"));
        assert!(hint.contains("game.shaders.surface"));
        assert!(hint.contains("fn screen(in: ScreenIn) -> vec3f {"));
        let text = "fn screen(in: ScreenIn) -> vec3f { return in.color; }";
        let hint = build(Hook::Surface, SURFACE, text)
            .unwrap_err()
            .hint
            .unwrap();
        assert!(hint.contains("game.shaders.screen"));
    }

    #[test]
    fn a_name_inside_a_comment_or_a_longer_name_is_not_the_function() {
        let text = "// fn screen(in: ScreenIn) -> vec3f {}\nfn screen2(in: ScreenIn) -> vec3f { return in.color; }\nfn myscreen() {}";
        assert!(screen_error(text).message.contains("no `screen` function"));
    }

    #[test]
    fn a_wrong_shape_is_reported_at_the_fn() {
        for (text, line, column) in [
            (
                "\n  fn screen(in: vec2f) -> vec3f { return vec3f(); }",
                2,
                3,
            ),
            ("fn screen(in: ScreenIn) -> vec4f { return vec4f(); }", 1, 1),
            ("\nfn screen() -> vec3f { return vec3f(); }", 2, 1),
            (
                "fn screen(in: ScreenIn, x: f32) -> vec3f { return vec3f(); }",
                1,
                1,
            ),
            ("fn screen(in: ScreenIn) { }", 1, 1),
        ] {
            let error = screen_error(text);
            assert_eq!(error.line, Some(line), "{text}");
            assert_eq!(error.column, Some(column), "{text}");
            assert!(
                error
                    .message
                    .contains("must take one `ScreenIn` and give back vec3f")
            );
            assert!(error.source_line.is_some());
            assert_eq!(
                error.hint.as_deref(),
                Some("Write its first line like this: `fn screen(in: ScreenIn) -> vec3f {`")
            );
        }
        let error = build(
            Hook::Surface,
            SURFACE,
            "fn surface(in: SurfaceIn) -> vec3f {}",
        )
        .unwrap_err();
        assert!(error.message.contains("`SurfaceIn` and give back Surface"));
    }

    #[test]
    fn spacing_comments_and_vec3_spelled_out_are_allowed() {
        for text in [
            "fn screen( in : ScreenIn , )\n  -> vec3<f32>\n  {\n return in.color; }",
            "fn /* a */ screen(/* b */ in: ScreenIn // c\n) -> vec3f /* d */ { return in.color; }",
        ] {
            let module = build(Hook::Screen, SCREEN, text).unwrap();
            compile(&module, Bounds::Clamped).unwrap();
        }
    }

    #[test]
    fn endless_loops_are_refused_before_naga() {
        for body in ["while (true) {}", "loop { break; }", "for (;;) { break; }"] {
            let text = format!(
                "fn screen(in: ScreenIn) -> vec3f {{\n    {body}\n    return in.color;\n}}"
            );
            let error = screen_error(&text);
            assert_eq!(error.line, Some(2));
            assert_eq!(error.column, Some(5));
            assert!(error.message.contains("freezes the picture"));
        }
        let fine = "// while (true) {}\nfn screen(in: ScreenIn) -> vec3f {\n    var loops = 0.0;\n    for (var i = 0; i < 4; i++) { loops += 1.0; }\n    return in.color * loops;\n}";
        let module = build(Hook::Screen, SCREEN, fine).unwrap();
        compile(&module, Bounds::Clamped).unwrap();
    }

    #[test]
    fn text_over_the_limit_is_refused() {
        let start = "fn screen(in: ScreenIn) -> vec3f { return in.color; }\n";
        let padding = "x".repeat(MAX_TEXT_BYTES - start.len() - 2);
        let make = |text: &str| {
            let block = ParamBlock::new(Vec::new());
            hooked_module(&template(SCREEN), Hook::Screen, &block, 1, 0, FILE, text)
        };
        assert!(make(&format!("{start}//{padding}")).is_ok());
        let error = make(&format!("{start}//{padding}xx")).unwrap_err();
        assert_eq!(error.file, FILE);
        assert!(error.message.contains("at most 64 KiB"), "{error}");
    }

    #[test]
    fn the_hook_names_and_starters() {
        for hook in [Hook::Screen, Hook::Surface] {
            let starter = hook.starter();
            assert_eq!(starter.lines().count(), 3);
            assert!(starter.starts_with(&format!("{} {{", hook.signature())));
            assert!(starter.starts_with(&format!("fn {}(", hook.entry())));
        }
    }
}
