//! Module text stitched from pieces, with a map back to each piece's file.

use crate::error::ShaderError;

/// A line that begins with this names another file whose text takes the
/// line's place: what several shaders share is written once.
const INCLUDE: &str = "//!include ";

/// Whose text a piece of a module is: who must fix a mistake in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// The text being compiled (a game's shader, or one of the engine's own
    /// shaders in build.rs): a mistake is reported plainly at its file and line.
    Author,
    /// Text LORE puts around a game's shader: a mistake in it is a fault in LORE.
    Lore,
}

/// A line's file, line number in that file, and owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place<'a> {
    /// The file the line came from.
    pub file: &'a str,
    /// The line's number in that file, counting from 1.
    pub line: u32,
    /// Whose text it is.
    pub owner: Owner,
}

/// A stretch of module lines that came from consecutive lines of one file.
#[derive(Clone, Copy, Debug)]
struct Run {
    /// The module line the run starts at, counting from 1.
    first: u32,
    /// How many module lines it holds.
    count: u32,
    /// Index into `Source::files`.
    file: usize,
    /// The line of that file the run starts at.
    file_line: u32,
    owner: Owner,
}

/// The text of one shader module, stitched from pieces, that remembers where
/// every line came from. Pieces always start on a new line, so a column in
/// the module is the same column in the piece's file.
#[derive(Clone, Debug, Default)]
pub struct Source {
    text: String,
    /// Lines in `text`.
    lines: u32,
    files: Vec<String>,
    runs: Vec<Run>,
}

impl Source {
    /// An empty module.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `text`, whose first line is line `first_line` of `file`.
    /// Line endings are normalised (`str::lines`, then `\n` after each line).
    pub fn push(&mut self, owner: Owner, file: &str, first_line: u32, text: &str) {
        let count = self.append(text);
        self.add_run(owner, file, first_line, count);
    }

    /// Adds the whole of `file` (whose text is `text`, line 1 onwards), with
    /// each line starting `//!include ` replaced by the text `load(name)`
    /// gives for the name after it (trimmed). One level deep: an include line
    /// inside included text is an error. A failed `load` is an error at the
    /// include line, holding the loader's reason.
    ///
    /// Included text goes in raw and a newline follows it, so an included
    /// file that ends in a newline (or is empty) leaves one empty line. That
    /// line counts as the include line of `file`. On an error the module is
    /// left as it was.
    pub fn push_with_includes(
        &mut self,
        owner: Owner,
        file: &str,
        text: &str,
        mut load: impl FnMut(&str) -> Result<String, String>,
    ) -> Result<(), ShaderError> {
        let before = (self.text.len(), self.lines, self.runs.len());
        let result = self.stitch(owner, file, text, &mut load);
        if result.is_err() {
            self.text.truncate(before.0);
            self.lines = before.1;
            self.runs.truncate(before.2);
        }
        result
    }

    /// The whole module's text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Where line `line` of the module (counting from 1) came from.
    pub fn place(&self, line: u32) -> Option<Place<'_>> {
        let after = self.runs.partition_point(|run| run.first <= line);
        let run = self.runs.get(after.checked_sub(1)?)?;
        let offset = line - run.first;
        (offset < run.count).then(|| Place {
            file: &self.files[run.file],
            line: run.file_line + offset,
            owner: run.owner,
        })
    }

    /// The first file pushed as the author's own, if any.
    pub(crate) fn first_author_file(&self) -> Option<&str> {
        let run = self.runs.iter().find(|run| run.owner == Owner::Author)?;
        Some(&self.files[run.file])
    }

    fn stitch(
        &mut self,
        owner: Owner,
        file: &str,
        text: &str,
        load: &mut impl FnMut(&str) -> Result<String, String>,
    ) -> Result<(), ShaderError> {
        // Plain lines are gathered into one run until an include breaks them.
        let mut plain: Option<(u32, u32)> = None;
        let flush = |this: &mut Self, plain: &mut Option<(u32, u32)>| {
            if let Some((first, count)) = plain.take() {
                this.add_run(owner, file, first, count);
            }
        };
        for (index, line) in text.lines().enumerate() {
            let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
            let Some(name) = line.strip_prefix(INCLUDE) else {
                self.text.push_str(line);
                self.text.push('\n');
                self.lines = self.lines.saturating_add(1);
                match &mut plain {
                    Some((_, count)) => *count += 1,
                    None => plain = Some((number, 1)),
                }
                continue;
            };
            let name = name.trim();
            let included = load(name).map_err(|reason| {
                ShaderError::plain(
                    file,
                    number,
                    Some(line),
                    format!("Could not include `{name}`: {reason}"),
                    Some(
                        "The name after `//!include` is a file that sits beside this one. \
                         Check its spelling and that the file exists."
                            .to_owned(),
                    ),
                )
            })?;
            if let Some(nested) = included
                .lines()
                .position(|inner| inner.starts_with(INCLUDE))
            {
                return Err(ShaderError::plain(
                    name,
                    u32::try_from(nested + 1).unwrap_or(u32::MAX),
                    included.lines().nth(nested),
                    format!(
                        "`{name}` has an `//!include` line, and an included file cannot include another."
                    ),
                    Some(
                        "Put the `//!include` line in the file that is being built instead."
                            .to_owned(),
                    ),
                ));
            }
            // The lines before the include are closed off first so the
            // included text lands in its place.
            flush(self, &mut plain);
            let count = self.append(&included);
            self.add_run(owner, name, 1, count);
            // The raw text is followed by a newline: after a file that ended
            // in one (or that is empty) that newline is an empty line of its own.
            if included.is_empty() || included.ends_with('\n') {
                self.text.push('\n');
                self.lines = self.lines.saturating_add(1);
                self.add_run(owner, file, number, 1);
            }
        }
        flush(self, &mut plain);
        Ok(())
    }

    /// Adds `text` line by line and says how many lines that was.
    fn append(&mut self, text: &str) -> u32 {
        let mut count = 0u32;
        for line in text.lines() {
            self.text.push_str(line);
            self.text.push('\n');
            count = count.saturating_add(1);
        }
        self.lines = self.lines.saturating_add(count);
        count
    }

    /// Records that the last `count` lines of the module are `file` from `first_line`.
    fn add_run(&mut self, owner: Owner, file: &str, first_line: u32, count: u32) {
        if count == 0 {
            return;
        }
        let file = match self.files.iter().position(|known| known == file) {
            Some(index) => index,
            None => {
                self.files.push(file.to_owned());
                self.files.len() - 1
            }
        };
        self.runs.push(Run {
            first: self.lines - count + 1,
            count,
            file,
            file_line: first_line,
            owner,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The expansion `crates/lore-render/build.rs` did before this crate
    /// existed, kept as the oracle the new one must match.
    fn old_with_includes(
        text: &str,
        load: &dyn Fn(&str) -> Result<String, String>,
    ) -> Result<String, String> {
        let mut whole = String::new();
        for line in text.lines() {
            if let Some(name) = line.strip_prefix("//!include ") {
                whole.push_str(&load(name.trim())?);
            } else {
                whole.push_str(line);
            }
            whole.push('\n');
        }
        Ok(whole)
    }

    fn loader(name: &str) -> Result<String, String> {
        match name {
            "one.wgsl" => Ok("a1\na2".to_owned()),
            "newline.wgsl" => Ok("b1\nb2\n".to_owned()),
            "empty.wgsl" => Ok(String::new()),
            "blank.wgsl" => Ok("\n".to_owned()),
            "nested.wgsl" => Ok("c1\n//!include one.wgsl\nc3\n".to_owned()),
            other => Err(format!("no file called {other}")),
        }
    }

    fn built(text: &str) -> Source {
        let mut source = Source::new();
        match source.push_with_includes(Owner::Author, "main.wgsl", text, loader) {
            Ok(()) => source,
            Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn text_matches_the_old_expansion() {
        let cases = [
            "x1\nx2\nx3\n",
            "x1\nx2\nx3",
            "",
            "x1\n//!include one.wgsl\nx3\n",
            "x1\n//!include newline.wgsl\nx3\n",
            "x1\n//!include empty.wgsl\nx3\n",
            "x1\n//!include blank.wgsl\nx3\n",
            "//!include one.wgsl\nx2\n",
            "x1\n//!include newline.wgsl",
            "//!include one.wgsl\n//!include newline.wgsl\n//!include empty.wgsl\nz\n//!include one.wgsl\n",
            "//!include  one.wgsl  \n//!includeone.wgsl\n",
        ];
        for case in cases {
            let wanted = old_with_includes(case, &|name| loader(name));
            let source = built(case);
            assert_eq!(Ok(source.text().to_owned()), wanted, "for {case:?}");
        }
    }

    #[test]
    fn place_maps_lines_across_pieces_and_includes() {
        let source = built("x1\n//!include newline.wgsl\nx3\n//!include empty.wgsl\nx5\n");
        // x1 b1 b2 (blank) x3 (blank) x5
        let places: Vec<(String, u32)> = (1..=7)
            .map(|line| {
                let place = source.place(line).expect("every line has a place");
                (place.file.to_owned(), place.line)
            })
            .collect();
        let expected = [
            ("main.wgsl", 1),
            ("newline.wgsl", 1),
            ("newline.wgsl", 2),
            ("main.wgsl", 2),
            ("main.wgsl", 3),
            ("main.wgsl", 4),
            ("main.wgsl", 5),
        ];
        for (got, want) in places.iter().zip(expected) {
            assert_eq!((got.0.as_str(), got.1), want);
        }
        assert_eq!(source.place(0), None);
        assert_eq!(source.place(8), None);
    }

    #[test]
    fn push_keeps_each_pieces_owner_and_first_line() {
        let mut source = Source::new();
        source.push(Owner::Lore, "engine/screen.wgsl", 10, "l1\nl2");
        source.push(Owner::Author, "shaders/heat.wgsl", 1, "a1\r\na2\r\n");
        assert_eq!(source.text(), "l1\nl2\na1\na2\n");
        assert_eq!(
            source.place(2),
            Some(Place {
                file: "engine/screen.wgsl",
                line: 11,
                owner: Owner::Lore
            })
        );
        assert_eq!(
            source.place(4),
            Some(Place {
                file: "shaders/heat.wgsl",
                line: 2,
                owner: Owner::Author
            })
        );
        assert_eq!(source.first_author_file(), Some("shaders/heat.wgsl"));
    }

    #[test]
    fn a_failed_load_is_an_error_at_the_include_line() {
        let mut source = Source::new();
        let error = source
            .push_with_includes(
                Owner::Author,
                "mesh.wgsl",
                "x1\nx2\n//!include missing.wgsl\nx4\n",
                loader,
            )
            .expect_err("the file does not exist");
        assert_eq!(error.file, "mesh.wgsl");
        assert_eq!(error.line, Some(3));
        assert_eq!(
            error.source_line.as_deref(),
            Some("//!include missing.wgsl")
        );
        assert!(error.message.contains("missing.wgsl"), "{error}");
        assert!(
            error.message.contains("no file called missing.wgsl"),
            "{error}"
        );
        assert!(error.hint.is_some());
        assert_eq!(source.text(), "", "nothing was kept");
    }

    #[test]
    fn an_include_inside_included_text_is_an_error() {
        let mut source = Source::new();
        let error = source
            .push_with_includes(
                Owner::Author,
                "mesh.wgsl",
                "x1\n//!include nested.wgsl\n",
                loader,
            )
            .expect_err("includes are one level deep");
        assert_eq!(error.file, "nested.wgsl");
        assert_eq!(error.line, Some(2));
        assert!(error.message.contains("cannot include another"), "{error}");
        assert_eq!(source.text(), "");
    }
}
