//! Checking a module with naga and writing SPIR-V, with errors a person can act on.

use std::error::Error;
use std::ops::Range;

use naga::back::spv;
use naga::proc::{BoundsCheckPolicies, BoundsCheckPolicy};
use naga::valid::{Capabilities, ValidationFlags, Validator};

use crate::error::{self, FAULT_HINT, ShaderError, Slip};
use crate::source::{Owner, Source};
use crate::spirv;

/// How array and buffer indexes are guarded in the SPIR-V.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bounds {
    /// Indexes are trusted (naga's `Unchecked`), as for the engine's own shaders.
    #[default]
    Trusted,
    /// Every index into an array or a buffer is kept inside what it indexes
    /// (naga's `Restrict` for index, buffer, binding_array), for a game's
    /// shaders. Image reads (`textureLoad`) are trusted as in the engine's
    /// own: naga 30 clamps one whose coordinates are unsigned with a vector
    /// constant of the wrong type, which is not valid SPIR-V (#434).
    Clamped,
}

/// A compiled module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    /// SPIR-V 1.3, ready for `vkCreateShaderModule`.
    pub words: Vec<u32>,
    /// The names of the module's entry points, in module order.
    pub entry_points: Vec<String>,
}

/// Parses, validates (`ValidationFlags::all()`, `Capabilities::IMMEDIATES`)
/// and writes SPIR-V 1.3, with the module's text as `source` gives it.
///
/// A mistake in the author's own text is reported at its file and line. A
/// mistake that only touches LORE's text is a fault in LORE, and says so.
pub fn compile(source: &Source, bounds: Bounds) -> Result<Compiled, ShaderError> {
    let text = source.text();
    let module = naga::front::wgsl::parse_str(text).map_err(|failure| {
        let labels: Vec<Label> = failure
            .labels()
            .map(|(span, what)| (span.to_range(), what.to_owned()))
            .collect();
        explain(source, failure.message(), &labels, "")
    })?;
    // Push constants carry the few numbers a pass needs, with no buffer to manage.
    let info = Validator::new(ValidationFlags::all(), Capabilities::IMMEDIATES)
        .validate(&module)
        .map_err(|failure| {
            let labels: Vec<Label> = failure
                .spans()
                .map(|(span, what)| (span.to_range(), what.clone()))
                .collect();
            // What naga really complains about is down the chain of causes.
            let mut cause: Option<&dyn Error> = Error::source(&failure);
            let mut detail = String::new();
            while let Some(next) = cause {
                if !detail.is_empty() {
                    detail.push_str(": ");
                }
                detail.push_str(&next.to_string());
                cause = next.source();
            }
            explain(source, &failure.to_string(), &labels, &detail)
        })?;
    let policy = match bounds {
        Bounds::Trusted => BoundsCheckPolicy::Unchecked,
        Bounds::Clamped => BoundsCheckPolicy::Restrict,
    };
    let options = spv::Options {
        // Vulkan 1.3 accepts SPIR-V up to 1.6; 1.3 is plenty and widely tested.
        lang_version: (1, 3),
        bounds_check_policies: BoundsCheckPolicies {
            index: policy,
            buffer: policy,
            image_load: BoundsCheckPolicy::Unchecked,
            binding_array: policy,
        },
        ..spv::Options::default()
    };
    let words = spv::write_vec(&module, &info, &options, None)
        .map_err(|failure| explain(source, &failure.to_string(), &[], ""))?;
    if let Some(what) = spirv::mistyped_constant(&words) {
        // Nothing the author wrote can cause this: what the compiler wrote
        // is wrong, and a driver must not be handed it.
        let file = source.first_author_file().unwrap_or("the shader");
        return Err(ShaderError::whole_file(
            file,
            format!("The compiled shader is not valid SPIR-V: {what}."),
            Some(FAULT_HINT.to_owned()),
        ));
    }
    Ok(Compiled {
        words,
        entry_points: module
            .entry_points
            .iter()
            .map(|entry| entry.name.clone())
            .collect(),
    })
}

/// A byte range of the module's text, when naga knew one, and what it says about it.
type Label = (Option<Range<usize>>, String);

/// Turns what naga said into a [`ShaderError`] at the right file and line.
/// `detail` is the chain of causes behind `message`, when there is one.
fn explain(source: &Source, message: &str, labels: &[Label], detail: &str) -> ShaderError {
    let text = source.text();
    let located: Vec<(usize, &str)> = labels
        .iter()
        .filter_map(|(range, what)| {
            let start = range.as_ref()?.start;
            text.is_char_boundary(start)
                .then_some((start, what.as_str()))
        })
        .collect();
    let owner_at = |start: usize| source.place(line_at(text, start)).map(|place| place.owner);

    let slip = error::recognise(message, detail);
    // Mistakes in a returned value are shown at the value, not at the
    // function that holds it: it is the last of naga's labels in the author's text.
    let wants_innermost = matches!(slip, Some(Slip::WrongReturn { .. }));
    let is_author = |label: &&(usize, &str)| owner_at(label.0) == Some(Owner::Author);
    let author = if wants_innermost {
        located.iter().rfind(is_author)
    } else {
        located.iter().find(is_author)
    };

    let what = match detail {
        "" => message.to_owned(),
        detail => format!("{message}: {detail}"),
    };
    let Some(&(start, _)) = author.or(located.first()) else {
        // No span at all: all that is known is which file was being built.
        let file = source.first_author_file().unwrap_or("the shader");
        return ShaderError::whole_file(file, what, None);
    };
    let line = line_at(text, start);
    let Some(place) = source.place(line) else {
        let file = source.first_author_file().unwrap_or("the shader");
        return ShaderError::whole_file(file, what, None);
    };
    let line_start = text[..start].rfind('\n').map_or(0, |at| at + 1);
    let line_text = text[line_start..].lines().next().unwrap_or_default();
    let column = Some(error::column_of(line_text, start - line_start));
    let source_line = Some(line_text.trim_end().to_owned());

    if place.owner == Owner::Lore {
        // Nothing the author wrote is at fault: the engine's own text is.
        let building = match source.first_author_file() {
            Some(file) => format!(" (LORE was building {file} when this happened.)"),
            None => String::new(),
        };
        return ShaderError {
            file: place.file.to_owned(),
            line: Some(place.line),
            column,
            message: format!("{what}{building}"),
            source_line,
            hint: Some(FAULT_HINT.to_owned()),
            note: None,
            lore_fault: true,
        };
    }

    let (message, hint) = match slip {
        Some(mut slip) => {
            if let Slip::Redefined { by_lore, .. } = &mut slip {
                // naga marks both declarations: if either is LORE's, the
                // maker took a name LORE already uses.
                *by_lore = located
                    .iter()
                    .any(|(at, _)| owner_at(*at) == Some(Owner::Lore));
            }
            let (message, hint) = error::explain(&slip, text, &text[..start]);
            (message, hint)
        }
        None => (what, None),
    };
    ShaderError {
        file: place.file.to_owned(),
        line: Some(place.line),
        column,
        message,
        source_line,
        hint,
        note: None,
        lore_fault: false,
    }
}

/// The module line (from 1) that byte `at` of `text` is on.
fn line_at(text: &str, at: usize) -> u32 {
    let newlines = text.as_bytes()[..at.min(text.len())]
        .iter()
        .filter(|&&byte| byte == b'\n')
        .count();
    u32::try_from(newlines + 1).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;

    /// A stand-in for what LORE puts in front of a screen effect.
    const HEAD: &str = "struct ScreenIn { uv: vec2f, color: vec3f, distance: f32, time: f32 }
struct Params { strength: f32 }
@group(0) @binding(0) var<uniform> params: Params;
fn luminance(c: vec3f) -> f32 { return dot(c, vec3f(0.2126, 0.7152, 0.0722)); }
";

    /// What LORE puts after it: the entry point that calls the author's `screen`.
    const TAIL: &str = "@fragment
fn main() -> @location(0) vec4f {
    var input: ScreenIn;
    return vec4f(screen(input), 1.0);
}
";

    const FILE: &str = "shaders/heat.wgsl";

    fn module_with(head: &str, author: &str) -> Source {
        let mut source = Source::new();
        source.push(Owner::Lore, "engine/screen.wgsl", 1, head);
        source.push(Owner::Author, FILE, 1, author);
        source.push(Owner::Lore, "engine/screen.wgsl", 50, TAIL);
        source
    }

    fn slip(author: &str) -> ShaderError {
        match compile(&module_with(HEAD, author), Bounds::Trusted) {
            Ok(_) => panic!("this should not compile:\n{author}"),
            Err(error) => error,
        }
    }

    fn column_of(author: &str, line: usize, find: &str) -> u32 {
        let text = author.lines().nth(line - 1).expect("the line exists");
        u32::try_from(text.find(find).expect("the text is on the line") + 1).unwrap()
    }

    #[test]
    fn a_good_module_compiles() {
        let good =
            "fn screen(in: ScreenIn) -> vec3f {\n    return in.color * luminance(in.color);\n}\n";
        let compiled = compile(&module_with(HEAD, good), Bounds::Trusted).expect("compiles");
        assert_eq!(compiled.entry_points, ["main"]);
        assert_eq!(compiled.words.first(), Some(&0x0723_0203), "SPIR-V magic");
        assert_eq!(compiled.words.get(1), Some(&0x0001_0300), "SPIR-V 1.3");
    }

    /// A head with a picture to read, as LORE's screen template has.
    const PICTURE_HEAD: &str =
        "struct ScreenIn { uv: vec2f, color: vec3f, distance: f32, time: f32 }
struct Params { strength: f32 }
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var picture: texture_2d<f32>;
";

    /// An image read whose coordinates are unsigned.
    const UNSIGNED_READ: &str = "fn screen(in: ScreenIn) -> vec3f {
    return textureLoad(picture, vec2u(in.uv * 100.0), 0).rgb;
}
";

    #[test]
    fn a_clamped_module_that_reads_an_image_by_unsigned_coordinates_is_valid() {
        let module = module_with(PICTURE_HEAD, UNSIGNED_READ);
        let clamped = compile(&module, Bounds::Clamped).expect("compiles");
        assert_eq!(spirv::mistyped_constant(&clamped.words), None);
    }

    /// Why `Clamped` leaves image reads alone (#434). When this fails, naga
    /// has been put right: image reads can be clamped again.
    #[test]
    fn naga_still_writes_a_mistyped_constant_for_a_clamped_unsigned_image_read() {
        let source = module_with(PICTURE_HEAD, UNSIGNED_READ);
        let module = naga::front::wgsl::parse_str(source.text()).expect("parses");
        let info = Validator::new(ValidationFlags::all(), Capabilities::IMMEDIATES)
            .validate(&module)
            .expect("validates");
        let options = spv::Options {
            lang_version: (1, 3),
            bounds_check_policies: BoundsCheckPolicies {
                image_load: BoundsCheckPolicy::Restrict,
                ..BoundsCheckPolicies::default()
            },
            ..spv::Options::default()
        };
        let words = spv::write_vec(&module, &info, &options, None).expect("writes");
        assert!(spirv::mistyped_constant(&words).is_some());
    }

    #[test]
    fn clamped_indexes_change_the_output() {
        let author = "fn screen(in: ScreenIn) -> vec3f {
    var table = array<f32, 4>(1.0, 2.0, 3.0, 4.0);
    let at = i32(in.time);
    return vec3f(table[at]);
}
";
        let module = module_with(HEAD, author);
        let trusted = compile(&module, Bounds::Trusted).expect("compiles");
        let clamped = compile(&module, Bounds::Clamped).expect("compiles");
        assert_ne!(trusted.words, clamped.words);
        assert_eq!(trusted.entry_points, clamped.entry_points);
    }

    #[test]
    fn a_misspelt_field_suggests_the_close_one() {
        let author = "fn screen(in: ScreenIn) -> vec3f {\n    return in.colour;\n}\n";
        let error = slip(author);
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(2));
        assert_eq!(error.column, Some(column_of(author, 2, "colour")));
        assert_eq!(error.message, "`ScreenIn` has no field `colour`.");
        assert_eq!(error.hint.as_deref(), Some("Did you mean `color`?"));
        assert_eq!(error.source_line.as_deref(), Some("    return in.colour;"));
        assert!(!error.lore_fault);
    }

    #[test]
    fn a_field_that_is_not_close_lists_the_fields() {
        let error = slip("fn screen(in: ScreenIn) -> vec3f {\n    return vec3f(in.banana);\n}\n");
        assert_eq!(error.line, Some(2));
        assert_eq!(
            error.hint.as_deref(),
            Some("The fields are: `uv`, `color`, `distance`, `time`.")
        );
    }

    #[test]
    fn a_value_that_is_not_declared_says_how_to_declare_it() {
        let author = "fn screen(in: ScreenIn) -> vec3f {\n    return in.color * params.x;\n}\n";
        let error = slip(author);
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(2));
        assert_eq!(error.column, Some(column_of(author, 2, "x;")));
        assert_eq!(error.message, "There is no value called `x` in `params`.");
        let hint = error.hint.expect("a hint");
        assert!(hint.contains("//! param x: f32 = 0"), "{hint}");
        assert!(hint.contains("top of the file"), "{hint}");
    }

    #[test]
    fn a_misspelt_value_suggests_the_declared_one() {
        let error =
            slip("fn screen(in: ScreenIn) -> vec3f {\n    return in.color * params.strenth;\n}\n");
        let hint = error.hint.expect("a hint");
        assert!(hint.starts_with("Did you mean `strength`?"), "{hint}");
    }

    #[test]
    fn the_wrong_return_type_is_shown_at_the_returned_value() {
        let author = "fn screen(in: ScreenIn) -> vec3f {\n    return vec4f(in.color, 1.0);\n}\n";
        let error = slip(author);
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(2));
        assert_eq!(error.column, Some(column_of(author, 2, "vec4f")));
        assert!(error.message.contains("`screen` returns"), "{error}");
        let hint = error.hint.expect("a hint");
        assert!(
            hint.contains("type after `->` must match what `return` gives"),
            "{hint}"
        );
    }

    #[test]
    fn a_missing_return_is_shown_at_the_function() {
        let author = "fn screen(in: ScreenIn) -> vec3f {\n    let a = 1.0;\n}\n";
        let error = slip(author);
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(1));
        assert_eq!(error.column, Some(1));
        assert!(
            error
                .message
                .contains("`screen` can finish without returning"),
            "{error}"
        );
        assert!(error.hint.expect("a hint").contains("Add `return"));
    }

    #[test]
    fn a_name_that_lore_uses_is_shown_at_the_makers_line() {
        let author = "fn luminance(c: vec3f) -> f32 { return 2.0; }\nfn screen(in: ScreenIn) -> vec3f {\n    return in.color;\n}\n";
        let error = slip(author);
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(1));
        assert_eq!(error.column, Some(column_of(author, 1, "luminance")));
        assert!(!error.lore_fault);
        assert_eq!(
            error.message,
            "The name `luminance` is already used by LORE."
        );
        let hint = error.hint.expect("a hint");
        assert!(
            hint.contains("LORE's own shader text already declares `luminance`"),
            "{hint}"
        );
        assert!(hint.contains("Pick another name"), "{hint}");
    }

    #[test]
    fn a_name_declared_twice_by_the_maker_is_not_blamed_on_lore() {
        let author = "fn twice() {}\nfn twice() {}\nfn screen(in: ScreenIn) -> vec3f {\n    return in.color;\n}\n";
        let error = slip(author);
        assert_eq!(error.line, Some(2));
        assert_eq!(error.message, "The name `twice` is declared twice.");
    }

    #[test]
    fn a_vector_without_that_component_says_so() {
        let author = "fn screen(in: ScreenIn) -> vec3f {\n    var c = in.color;\n    c.w = 1.0;\n    return c;\n}\n";
        let error = slip(author);
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(3));
        assert_eq!(error.message, "`c` has no `.w`.");
        let hint = error.hint.expect("a hint");
        assert!(hint.contains("`vec2f` has x and y"), "{hint}");
        assert!(hint.contains("only a `vec4f` has w"), "{hint}");
        let error = slip("fn screen(in: ScreenIn) -> vec3f {\n    return vec3f(in.color.a);\n}\n");
        assert_eq!(error.message, "`color` has no `.a`.");
        assert!(!error.hint.expect("a hint").contains("`uv`"));
    }

    #[test]
    fn a_misspelt_name_after_a_dot_on_a_value_says_to_check_it() {
        let error = slip("fn screen(in: ScreenIn) -> vec3f {\n    return in.color.banana;\n}\n");
        assert_eq!(error.message, "`color` has no field `banana`.");
        assert_eq!(
            error.hint.as_deref(),
            Some("Check the spelling of the name after the `.`.")
        );
    }

    #[test]
    fn a_number_where_a_vector_is_needed_shows_how_to_make_one() {
        for author in [
            "fn screen(in: ScreenIn) -> vec3f {\n    return 1.0;\n}\n",
            "fn screen(in: ScreenIn) -> vec3f {\n    let c: vec3f = 0.5;\n    return c;\n}\n",
        ] {
            let error = slip(author);
            assert_eq!(error.file, FILE, "{error}");
            assert_eq!(
                error.message, "This gives a number where a `vec3f` is needed.",
                "{error}"
            );
            let hint = error.hint.expect("a hint");
            assert!(hint.contains("`vec3f(0.5)`"), "{hint}");
            assert!(hint.contains("`vec3f(r, g, b)`"), "{hint}");
            assert!(!error.lore_fault);
        }
        let error = slip(
            "fn screen(in: ScreenIn) -> vec3f {\n    let c: vec3f = in.uv;\n    return c;\n}\n",
        );
        assert_eq!(
            error.message,
            "This gives a `vec2f` where a `vec3f` is needed."
        );
    }

    #[test]
    fn a_name_that_is_not_declared_suggests_the_close_one() {
        let author = "fn screen(in: ScreenIn) -> vec3f {\n    let strength = 2.0;\n    return in.color * strenght;\n}\n";
        let error = slip(author);
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(3));
        assert_eq!(error.column, Some(column_of(author, 3, "strenght")));
        assert_eq!(error.message, "Nothing called `strenght` is declared.");
        assert_eq!(error.hint.as_deref(), Some("Did you mean `strength`?"));
        let error =
            slip("fn screen(in: ScreenIn) -> vec3f {\n    return in.color * foo(1.0);\n}\n");
        assert_eq!(error.message, "Nothing called `foo` is declared.");
        let hint = error.hint.expect("a hint");
        assert!(hint.contains("declared before it is used"), "{hint}");
    }

    #[test]
    fn a_missing_semicolon_says_to_look_above() {
        let author =
            "fn screen(in: ScreenIn) -> vec3f {\n    let a = 1.0\n    return in.color;\n}\n";
        let error = slip(author);
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(3));
        assert_eq!(error.column, Some(column_of(author, 3, "return")));
        let hint = error.hint.expect("a hint");
        assert!(hint.contains("Each statement ends with `;`"), "{hint}");
        assert!(hint.contains("end of the line above"), "{hint}");
    }

    #[test]
    fn an_error_without_a_hint_keeps_nagas_message() {
        let error = slip("fn screen(in: ScreenIn) -> vec3f {\n    return in.color + 1.0f2;\n}\n");
        assert_eq!(error.file, FILE);
        assert_eq!(error.line, Some(2));
        assert!(!error.message.is_empty());
    }

    #[test]
    fn a_mistake_in_lores_text_is_a_fault_in_lore() {
        let head = format!("{HEAD}fn broken() -> f32 {{ return vec3f(1.0); }}\n");
        let author = "fn screen(in: ScreenIn) -> vec3f {\n    return in.color;\n}\n";
        let error = compile(&module_with(&head, author), Bounds::Trusted).expect_err("broken");
        assert!(error.lore_fault);
        assert_eq!(error.file, "engine/screen.wgsl");
        assert_eq!(error.line, Some(5));
        assert!(error.message.contains(FILE), "{error}");
        assert_eq!(error.hint.as_deref(), Some(FAULT_HINT));
        assert!(
            error
                .to_string()
                .starts_with("Shader error in LORE's own engine/screen.wgsl, line 5:")
        );
    }

    #[test]
    fn the_whole_error_reads_well() {
        let error = slip("fn screen(in: ScreenIn) -> vec3f {\n    return in.colour;\n}\n")
            .with_note("The last version that worked is still drawn.");
        assert_eq!(
            error.to_string(),
            "Shader error in shaders/heat.wgsl, line 2:\n\n     \
             2 | return in.colour;\n       \
             |           ^\n\n  \
             `ScreenIn` has no field `colour`.\n  \
             Hint: Did you mean `color`?\n  \
             The last version that worked is still drawn."
        );
    }

    fn shaders_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lore-render/shaders")
    }

    fn engine_module(name: &str) -> Result<Source, ShaderError> {
        let dir = shaders_dir();
        let text = fs::read_to_string(dir.join(format!("{name}.wgsl"))).expect("shader file");
        let mut source = Source::new();
        source.push_with_includes(
            Owner::Author,
            &format!("shaders/{name}.wgsl"),
            &text,
            |included| fs::read_to_string(dir.join(included)).map_err(|error| error.to_string()),
        )?;
        Ok(source)
    }

    #[test]
    fn all_the_engines_shaders_compile() {
        for name in ["eye", "grid", "mesh", "ripples", "tonemap", "ui", "waves"] {
            let source = engine_module(name).unwrap_or_else(|error| panic!("{error}"));
            let compiled =
                compile(&source, Bounds::Trusted).unwrap_or_else(|error| panic!("{error}"));
            assert!(!compiled.entry_points.is_empty(), "{name} has entry points");
        }
    }

    #[test]
    #[ignore = "times the compile of the biggest shader: run it with --nocapture"]
    fn mesh_compile_time() {
        let source = engine_module("mesh").expect("mesh stitches");
        let mut times: Vec<std::time::Duration> = (0..5)
            .map(|_| {
                let start = std::time::Instant::now();
                compile(&source, Bounds::Trusted).expect("mesh compiles");
                start.elapsed()
            })
            .collect();
        times.sort();
        eprintln!("mesh.wgsl compile, median of 5: {:?}", times[2]);
    }
}
