//! The strict HTML parser.
//!
//! It reads the subset described in the crate docs and stops at the first
//! mistake with an error that carries file, line, column and what to write
//! instead. Tags and attributes are lowercase. Every tag that is not void is
//! closed by hand and properly nested.

use crate::dom::{
    Document, ElementId, FormState, HandlerKind, InputType, NodeKind, SourceText, Tag,
};
use crate::error::{MarkupError, name_help};

/// The text of one `<style>` block and where it starts in the HTML file, so
/// a CSS error can point into that file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleBlock {
    /// The CSS, exactly as written.
    pub text: String,
    /// Line of the first character of `text`.
    pub line: u32,
    /// Column of the first character of `text`.
    pub column: u32,
    /// The `<style>` element.
    pub element: ElementId,
}

/// A `<link rel="stylesheet" href="...">`. The caller loads the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StylesheetLink {
    /// The `href` value.
    pub href: String,
    /// Line of the `<link`.
    pub line: u32,
    /// Column of the `<link`.
    pub column: u32,
}

/// Everything a page gives back.
#[derive(Debug)]
pub struct Parsed {
    /// The tree.
    pub document: Document,
    /// `<style>` blocks in document order.
    pub styles: Vec<StyleBlock>,
    /// Stylesheet links in document order.
    pub links: Vec<StylesheetLink>,
}

/// Parses a whole page. `file` is only used in error messages.
pub fn parse(file: &str, source: &str) -> Result<Parsed, MarkupError> {
    let mut document = Document::new();
    let root = document.root();
    let mut p = Parser::new(file, source, &mut document, root, Mode::Page);
    p.run()?;
    let (styles, links) = (p.styles, p.links);
    Ok(Parsed {
        document,
        styles,
        links,
    })
}

/// Parses content tags and adds them as the last children of `parent`.
/// `html`, `head`, `body`, `title`, `style` and `link` are not allowed here.
/// On error nothing is added. Returns the new top-level nodes.
pub fn parse_fragment(
    doc: &mut Document,
    parent: ElementId,
    file: &str,
    source: &str,
) -> Result<Vec<ElementId>, MarkupError> {
    let accepts = doc
        .get(parent)
        .is_some_and(|n| !matches!(n.kind, NodeKind::Text(_)));
    if !accepts {
        return Err(MarkupError::new(
            file,
            1,
            1,
            "cannot add HTML here: the target is text or no longer exists",
            "add it to an element instead",
        ));
    }
    let before = doc.last_child(parent);
    let base = doc.get(parent).and_then(|n| n.tag());
    let result = {
        let mut p = Parser::new(file, source, doc, parent, Mode::Fragment);
        p.base = base;
        p.run()
    };
    let mut added = Vec::new();
    let mut cur = match before {
        Some(b) => doc.next_sibling(b),
        None => doc.first_child(parent),
    };
    while let Some(c) = cur {
        added.push(c);
        cur = doc.next_sibling(c);
    }
    if let Err(e) = result {
        for c in added {
            doc.remove(c);
        }
        if base == Some(Tag::Select) {
            doc.refresh_select(parent);
        }
        return Err(e);
    }
    if base == Some(Tag::Select) {
        doc.refresh_select(parent);
    }
    Ok(added)
}

/// Gives `element` exactly the attributes in `attrs`, checked as the parser
/// checks them (allowed names for the tag, value shapes, unique ids). On an
/// error the element is unchanged. Form state is rebuilt from the
/// attributes. A boolean attribute is written with an empty value.
pub(crate) fn apply_attributes(
    doc: &mut Document,
    element: ElementId,
    file: &str,
    attrs: &[(String, String)],
) -> Result<(), MarkupError> {
    let Some(tag) = doc.get(element).and_then(crate::dom::Node::tag) else {
        return Err(MarkupError::new(
            file,
            1,
            1,
            "this is text or no longer exists, so it has no attributes",
            "find the element again",
        ));
    };
    let raw: Vec<RawAttr> = attrs
        .iter()
        .map(|(name, value)| RawAttr {
            name: name.clone(),
            value: if BOOLEAN_ATTRS.contains(&name.as_str()) {
                None
            } else {
                Some(value.clone())
            },
            line: 1,
            col: 1,
            vline: 1,
            vcol: 1,
        })
        .collect();
    let new_id = raw
        .iter()
        .find(|a| a.name == "id")
        .and_then(|a| a.value.clone());
    let mut p = Parser::new(file, "", doc, element, Mode::Fragment);
    p.fill(element, tag, &raw, (1, 1))?;
    let atom = new_id.map(|v| doc.intern(&v));
    doc.set_id(element, atom);
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Page,
    Fragment,
}

struct Open {
    id: ElementId,
    tag: Tag,
    line: u32,
    col: u32,
}

struct RawAttr {
    name: String,
    value: Option<String>,
    line: u32,
    col: u32,
    vline: u32,
    vcol: u32,
}

const GLOBAL_ATTRS: [&str; 11] = [
    "id",
    "class",
    "style",
    "hidden",
    "disabled",
    "tabindex",
    "autofocus",
    "onclick",
    "onchange",
    "oninput",
    "onfocus",
];
const BOOLEAN_ATTRS: [&str; 5] = ["hidden", "disabled", "autofocus", "checked", "selected"];
const ENTITIES: [(&str, char); 6] = [
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{a0}'),
];

fn tag_attrs(tag: Tag) -> &'static [&'static str] {
    match tag {
        Tag::Img => &["src", "alt"],
        Tag::Input => &[
            "type",
            "name",
            "value",
            "placeholder",
            "maxlength",
            "min",
            "max",
            "step",
            "checked",
        ],
        Tag::Select => &["name"],
        Tag::Option => &["value", "selected"],
        Tag::Label => &["for"],
        Tag::Progress => &["value", "max"],
        Tag::Link => &["rel", "href"],
        _ => &[],
    }
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn tag_ref(tag: Tag) -> String {
    format!("`<{}>`", tag.name())
}

struct Parser<'a> {
    file: &'a str,
    chars: Vec<char>,
    i: usize,
    line: u32,
    col: u32,
    doc: &'a mut Document,
    parent: ElementId,
    mode: Mode,
    base: Option<Tag>,
    stack: Vec<Open>,
    text: String,
    text_pos: (u32, u32),
    last_space: bool,
    texts: Vec<ElementId>,
    styles: Vec<StyleBlock>,
    links: Vec<StylesheetLink>,
    seen_html: bool,
    seen_head: bool,
    seen_body: bool,
    seen_doctype: bool,
}

impl<'a> Parser<'a> {
    fn new(
        file: &'a str,
        source: &str,
        doc: &'a mut Document,
        parent: ElementId,
        mode: Mode,
    ) -> Self {
        Self {
            file,
            chars: source.chars().collect(),
            i: 0,
            line: 1,
            col: 1,
            doc,
            parent,
            mode,
            base: None,
            stack: Vec::new(),
            text: String::new(),
            text_pos: (1, 1),
            last_space: false,
            texts: Vec::new(),
            styles: Vec::new(),
            links: Vec::new(),
            seen_html: false,
            seen_head: false,
            seen_body: false,
            seen_doctype: false,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.i).copied()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.i + n).copied()
    }

    fn pos(&self) -> (u32, u32) {
        (self.line, self.col)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.i).copied()?;
        self.i += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn starts_with(&self, s: &str) -> bool {
        s.chars()
            .enumerate()
            .all(|(k, c)| self.chars.get(self.i + k) == Some(&c))
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(is_ws) {
            self.bump();
        }
    }

    fn err(
        &self,
        at: (u32, u32),
        message: impl Into<String>,
        help: impl Into<String>,
    ) -> MarkupError {
        MarkupError::new(self.file, at.0, at.1, message, help)
    }

    /// The tag whose content we are in; `None` at the top.
    fn context(&self) -> Option<Tag> {
        match self.stack.last() {
            Some(o) => Some(o.tag),
            None => self.base,
        }
    }

    fn current_parent(&self) -> ElementId {
        self.stack.last().map_or(self.parent, |o| o.id)
    }

    fn run(&mut self) -> Result<(), MarkupError> {
        while let Some(c) = self.peek() {
            if c == '<' {
                self.markup()?;
            } else {
                self.text_char(c)?;
            }
        }
        self.flush_text()?;
        if let Some(open) = self.stack.last() {
            let name = open.tag.name();
            return Err(self.err(
                (open.line, open.col),
                format!("`<{name}>` is never closed"),
                format!("add `</{name}>` before the end of the file"),
            ));
        }
        self.drop_blank_text();
        Ok(())
    }

    // ----- text -----

    fn text_char(&mut self, c: char) -> Result<(), MarkupError> {
        if self.text.is_empty() {
            self.text_pos = self.pos();
        }
        if c == '&' {
            let ch = self.entity()?;
            self.text.push(ch);
            self.last_space = false;
        } else {
            self.bump();
            if is_ws(c) {
                if !self.last_space {
                    self.text.push(' ');
                    self.last_space = true;
                }
            } else {
                self.text.push(c);
                self.last_space = false;
            }
        }
        Ok(())
    }

    fn flush_text(&mut self) -> Result<(), MarkupError> {
        if self.text.is_empty() {
            return Ok(());
        }
        let text = std::mem::take(&mut self.text);
        self.last_space = false;
        let blank = text.chars().all(is_ws);
        let structural = match self.context() {
            Some(Tag::Html | Tag::Head | Tag::Select) => true,
            None => self.mode == Mode::Page && self.seen_html,
            _ => false,
        };
        if structural {
            if blank {
                return Ok(());
            }
            let place = match self.context() {
                Some(t) => format!("directly inside `<{}>`", t.name()),
                None => "after `</html>`".to_owned(),
            };
            let help = match self.context() {
                Some(Tag::Select) => "put each choice in an `<option>`",
                Some(Tag::Head) => {
                    "put text in the `<body>`; the `<head>` holds title, style and link"
                }
                _ => "put text inside `<body>`",
            };
            return Err(self.err(self.text_pos, format!("text is not allowed {place}"), help));
        }
        let node = self
            .doc
            .create_text(&text, self.text_pos.0, self.text_pos.1);
        let parent = self.current_parent();
        if self.doc.append_child(parent, node) {
            self.texts.push(node);
        }
        Ok(())
    }

    /// Whitespace-only text survives only between two inline neighbours.
    fn drop_blank_text(&mut self) {
        let inline = |doc: &Document, id: Option<ElementId>| -> bool {
            id.and_then(|i| doc.get(i)).is_some_and(|n| match &n.kind {
                NodeKind::Text(_) => true,
                NodeKind::Element(t) => matches!(t, Tag::Span | Tag::Label),
                NodeKind::Root => false,
            })
        };
        for id in std::mem::take(&mut self.texts) {
            let blank = self
                .doc
                .get(id)
                .and_then(|n| n.text())
                .is_some_and(|t| t.chars().all(is_ws));
            if !blank {
                continue;
            }
            let keep = inline(self.doc, self.doc.prev_sibling(id))
                && inline(self.doc, self.doc.next_sibling(id));
            if !keep {
                self.doc.remove(id);
            }
        }
    }

    fn entity(&mut self) -> Result<char, MarkupError> {
        let at = self.pos();
        self.bump();
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if (c.is_ascii_alphanumeric() || c == '#') && name.len() < 12 {
                name.push(c);
                self.bump();
            } else {
                break;
            }
        }
        if self.peek() != Some(';') {
            let message = if name.is_empty() {
                "a bare `&`".to_owned()
            } else {
                format!("entity `&{name}` is missing its `;`")
            };
            return Err(self.err(
                at,
                message,
                "write `&amp;` to show an ampersand; other entities are &lt; &gt; &quot; &apos; &nbsp; &#NN; &#xHH;",
            ));
        }
        self.bump();
        if let Some(num) = name.strip_prefix('#') {
            let value = match num.strip_prefix('x') {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => num.parse::<u32>().ok(),
            };
            let ch = value.filter(|&v| v != 0).and_then(char::from_u32);
            return ch.ok_or_else(|| {
                self.err(
                    at,
                    format!("`&{name};` is not a character"),
                    "write a number like &#65; or &#x41;",
                )
            });
        }
        if let Some((_, c)) = ENTITIES.iter().find(|(n, _)| *n == name) {
            return Ok(*c);
        }
        let names: Vec<&str> = ENTITIES.iter().map(|(n, _)| *n).collect();
        Err(self.err(
            at,
            format!("unknown entity `&{name};`"),
            name_help(&name, &names),
        ))
    }

    // ----- markup -----

    fn markup(&mut self) -> Result<(), MarkupError> {
        if self.starts_with("<!--") {
            return self.comment();
        }
        self.flush_text()?;
        if self.starts_with("<!") {
            return self.declaration();
        }
        if self.starts_with("</") {
            return self.close_tag();
        }
        self.open_tag()
    }

    fn comment(&mut self) -> Result<(), MarkupError> {
        let at = self.pos();
        for _ in 0..4 {
            self.bump();
        }
        while self.peek().is_some() {
            if self.starts_with("-->") {
                for _ in 0..3 {
                    self.bump();
                }
                return Ok(());
            }
            self.bump();
        }
        Err(self.err(at, "this comment is never closed", "end it with `-->`"))
    }

    fn declaration(&mut self) -> Result<(), MarkupError> {
        let at = self.pos();
        let mut inner = String::new();
        self.bump();
        loop {
            match self.bump() {
                None => {
                    return Err(self.err(at, "this `<!` is never closed", "end it with `>`"));
                }
                Some('>') => break,
                Some(c) => inner.push(c),
            }
        }
        let is_doctype = inner.trim_end().eq_ignore_ascii_case("!doctype html");
        let first = self.mode == Mode::Page
            && !self.seen_doctype
            && self.stack.is_empty()
            && self
                .doc
                .children(self.parent)
                .all(|c| self.doc.get(c).is_some_and(|n| n.text().is_some()));
        if is_doctype && first {
            self.seen_doctype = true;
            return Ok(());
        }
        if is_doctype {
            return Err(self.err(
                at,
                "`<!DOCTYPE html>` is only allowed once, at the very top of a page",
                "remove it here",
            ));
        }
        Err(self.err(
            at,
            format!("unsupported declaration `<{inner}>`"),
            "write `<!DOCTYPE html>` at the top, or `<!-- like this -->` for a comment",
        ))
    }

    fn read_name(&mut self) -> String {
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '-' {
                name.push(c);
                self.bump();
            } else {
                break;
            }
        }
        name
    }

    fn unterminated(&self, at: (u32, u32), shown: &str) -> MarkupError {
        self.err(
            at,
            format!("the tag `{shown}` is never finished"),
            "end it with `>`",
        )
    }

    fn close_tag(&mut self) -> Result<(), MarkupError> {
        let at = self.pos();
        self.bump();
        self.bump();
        let name_at = self.pos();
        let name = self.read_name();
        if name.is_empty() {
            return Err(self.err(
                at,
                "expected a tag name after `</`",
                "write the name of the tag to close, like `</div>`",
            ));
        }
        self.skip_ws();
        match self.peek() {
            Some('>') => {
                self.bump();
            }
            None => return Err(self.unterminated(at, &format!("</{name}"))),
            Some(c) => {
                return Err(self.err(
                    self.pos(),
                    format!("found `{c}` where `>` should end `</{name}`"),
                    format!("write `</{name}>`"),
                ));
            }
        }
        let Some(tag) = Tag::from_name(&name) else {
            return Err(self.err(
                name_at,
                format!("unknown tag `{name}`"),
                name_help(&name, &Tag::NAMES),
            ));
        };
        if tag.is_void() {
            return Err(self.err(
                at,
                format!("{} has no closing tag", tag_ref(tag)),
                format!("remove `</{name}>`; write `<{name}>` or `<{name}/>` on its own"),
            ));
        }
        let Some(top) = self.stack.last() else {
            return Err(self.err(
                at,
                format!("`</{name}>` closes a tag that was never opened"),
                format!("remove it, or add the missing `<{name}>` before it"),
            ));
        };
        if top.tag != tag {
            let open = top.tag.name();
            return Err(self.err(
                at,
                format!(
                    "`</{name}>` found, but `<{open}>` opened at line {}, column {} is still open",
                    top.line, top.col
                ),
                format!("close `<{open}>` first with `</{open}>`"),
            ));
        }
        if let Some(open) = self.stack.pop()
            && open.tag == Tag::Select
        {
            self.doc.refresh_select(open.id);
        }
        Ok(())
    }

    fn open_tag(&mut self) -> Result<(), MarkupError> {
        let at = self.pos();
        self.bump();
        let name_at = self.pos();
        if !self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            return Err(self.err(
                at,
                "expected a tag name after `<`",
                "to show a `<` in text write `&lt;`",
            ));
        }
        let name = self.read_name();
        let Some(tag) = Tag::from_name(&name) else {
            return Err(self.err(
                name_at,
                format!("unknown tag `{name}`"),
                name_help(&name, &Tag::NAMES),
            ));
        };
        self.check_placement(tag, at)?;
        let (attrs, self_closed) = self.attributes(at, &name)?;
        if self_closed && !tag.is_void() {
            return Err(self.err(
                at,
                format!("`<{name}/>` cannot close itself"),
                format!("write `<{name}></{name}>`"),
            ));
        }
        let id = self.build_element(tag, &attrs, at)?;
        let parent = self.current_parent();
        if !self.doc.append_child(parent, id) {
            self.doc.remove(id);
            return Err(self.err(at, "this tag cannot be added here", "check the nesting"));
        }
        match tag {
            Tag::Html => self.seen_html = true,
            Tag::Head => self.seen_head = true,
            Tag::Body => self.seen_body = true,
            _ => {}
        }
        if tag == Tag::Link {
            let href = attrs
                .iter()
                .find(|a| a.name == "href")
                .and_then(|a| a.value.clone())
                .unwrap_or_default();
            self.links.push(StylesheetLink {
                href,
                line: at.0,
                column: at.1,
            });
        }
        match tag {
            _ if tag.is_void() => {}
            Tag::Style | Tag::Title => self.raw_text(tag, id, at)?,
            _ => self.stack.push(Open {
                id,
                tag,
                line: at.0,
                col: at.1,
            }),
        }
        Ok(())
    }

    /// Content of `<style>` and `<title>` is not markup: read to the close.
    fn raw_text(
        &mut self,
        tag: Tag,
        id: ElementId,
        open_at: (u32, u32),
    ) -> Result<(), MarkupError> {
        let close = format!("</{}>", tag.name());
        let start = self.pos();
        let mut text = String::new();
        loop {
            if self.starts_with(&close) {
                for _ in 0..close.chars().count() {
                    self.bump();
                }
                break;
            }
            match self.bump() {
                Some(c) => text.push(c),
                None => {
                    return Err(self.err(
                        open_at,
                        format!("`<{}>` is never closed", tag.name()),
                        format!("add `{close}` after its content"),
                    ));
                }
            }
        }
        if tag == Tag::Style {
            self.styles.push(StyleBlock {
                text,
                line: start.0,
                column: start.1,
                element: id,
            });
        }
        Ok(())
    }

    fn check_placement(&self, tag: Tag, at: (u32, u32)) -> Result<(), MarkupError> {
        let ctx = self.context();
        let name = tag.name();
        if self.mode == Mode::Fragment
            && matches!(
                tag,
                Tag::Html | Tag::Head | Tag::Body | Tag::Title | Tag::Style | Tag::Link
            )
        {
            return Err(self.err(
                at,
                format!("`<{name}>` cannot be added to a page that is already showing"),
                "add content tags only; put styles in the page's `<head>`",
            ));
        }
        let bad = |message: String, help: &str| Err(self.err(at, message, help));
        match tag {
            Tag::Html if ctx.is_some() || self.seen_html => {
                return bad(
                    "`<html>` must be the outermost tag, and appear once".into(),
                    "remove this one",
                );
            }
            Tag::Head if ctx != Some(Tag::Html) || self.seen_head => {
                return bad(
                    "`<head>` must be directly inside `<html>`, once".into(),
                    "put it first inside `<html>`",
                );
            }
            Tag::Body if ctx != Some(Tag::Html) || self.seen_body => {
                return bad(
                    "`<body>` must be directly inside `<html>`, once".into(),
                    "put it after `<head>` inside `<html>`",
                );
            }
            Tag::Title if ctx != Some(Tag::Head) => {
                return bad(
                    "`<title>` belongs in `<head>`".into(),
                    "move it into `<head>`",
                );
            }
            Tag::Option if ctx != Some(Tag::Select) => {
                return bad(
                    "`<option>` belongs directly inside `<select>`".into(),
                    "put it inside a `<select>`",
                );
            }
            _ => {}
        }
        match ctx {
            Some(Tag::Html) if !matches!(tag, Tag::Head | Tag::Body) => bad(
                format!("`<{name}>` cannot sit directly inside `<html>`"),
                "put it inside `<body>`",
            ),
            Some(Tag::Head) if !matches!(tag, Tag::Title | Tag::Style | Tag::Link) => bad(
                format!("`<{name}>` does not belong in `<head>`"),
                "the `<head>` holds `<title>`, `<style>` and `<link>`; move it to `<body>`",
            ),
            Some(Tag::Select) if tag != Tag::Option => bad(
                format!("`<{name}>` cannot sit inside `<select>`"),
                "a `<select>` holds only `<option>` tags",
            ),
            Some(Tag::Option) => bad(
                format!("`<{name}>` cannot sit inside `<option>`"),
                "an `<option>` holds only text",
            ),
            None if self.mode == Mode::Page && self.seen_html => bad(
                format!("`<{name}>` comes after `</html>`"),
                "put everything inside `<html>`",
            ),
            _ => Ok(()),
        }
    }

    // ----- attributes -----

    fn attributes(
        &mut self,
        tag_at: (u32, u32),
        tag_name: &str,
    ) -> Result<(Vec<RawAttr>, bool), MarkupError> {
        let shown = format!("<{tag_name}");
        let mut attrs = Vec::new();
        loop {
            self.skip_ws();
            let Some(c) = self.peek() else {
                return Err(self.unterminated(tag_at, &shown));
            };
            match c {
                '>' => {
                    self.bump();
                    return Ok((attrs, false));
                }
                '/' => {
                    if self.peek_at(1) == Some('>') {
                        self.bump();
                        self.bump();
                        return Ok((attrs, true));
                    }
                    return Err(self.err(
                        self.pos(),
                        "`/` is only allowed as `/>` at the end of a tag",
                        "write `<br/>` or `<br>`",
                    ));
                }
                c if c.is_ascii_alphabetic() => attrs.push(self.attribute(tag_at, &shown)?),
                c => {
                    return Err(self.err(
                        self.pos(),
                        format!("unexpected `{c}` inside the tag `{shown}`"),
                        "attribute names are lowercase letters, digits and `-`; write values in quotes, like class=\"big\"",
                    ));
                }
            }
        }
    }

    fn attribute(&mut self, tag_at: (u32, u32), shown: &str) -> Result<RawAttr, MarkupError> {
        let (line, col) = self.pos();
        let mut name = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.') {
                name.push(c);
                self.bump();
            } else {
                break;
            }
        }
        self.skip_ws();
        let mut attr = RawAttr {
            name,
            value: None,
            line,
            col,
            vline: line,
            vcol: col,
        };
        if self.peek() != Some('=') {
            return Ok(attr);
        }
        let eq_at = self.pos();
        self.bump();
        self.skip_ws();
        let Some(first) = self.peek() else {
            return Err(self.unterminated(tag_at, shown));
        };
        let mut value = String::new();
        if first == '"' || first == '\'' {
            let quote_at = self.pos();
            self.bump();
            (attr.vline, attr.vcol) = self.pos();
            loop {
                match self.peek() {
                    None => {
                        return Err(self.err(
                            quote_at,
                            "this attribute value is never closed",
                            format!("end it with a matching {first}"),
                        ));
                    }
                    Some(c) if c == first => {
                        self.bump();
                        break;
                    }
                    Some('&') => value.push(self.entity()?),
                    Some(c) => {
                        value.push(c);
                        self.bump();
                    }
                }
            }
        } else {
            if first == '>' {
                return Err(self.missing_value(eq_at, &attr.name));
            }
            (attr.vline, attr.vcol) = self.pos();
            while let Some(c) = self.peek() {
                if is_ws(c) || c == '>' || (c == '/' && self.peek_at(1) == Some('>')) {
                    break;
                }
                if matches!(c, '"' | '\'' | '=' | '<' | '`') {
                    return Err(self.err(
                        self.pos(),
                        format!("`{c}` is not allowed in a value without quotes"),
                        format!("put quotes around the value: {}=\"...\"", attr.name),
                    ));
                }
                if c == '&' {
                    value.push(self.entity()?);
                } else {
                    value.push(c);
                    self.bump();
                }
            }
            if self.peek().is_none() {
                return Err(self.unterminated(tag_at, shown));
            }
            if value.is_empty() {
                return Err(self.missing_value(eq_at, &attr.name));
            }
        }
        attr.value = Some(value);
        Ok(attr)
    }

    fn attr_err(
        &self,
        a: &RawAttr,
        message: impl Into<String>,
        help: impl Into<String>,
    ) -> MarkupError {
        self.err((a.line, a.col), message, help)
    }

    fn check_allowed(&self, tag: Tag, a: &RawAttr) -> Result<(), MarkupError> {
        let name = a.name.as_str();
        if name.len() > 5 && name.starts_with("data-") {
            return Ok(());
        }
        if GLOBAL_ATTRS.contains(&name) || tag_attrs(tag).contains(&name) {
            return Ok(());
        }
        let owners: Vec<String> = Tag::ALL
            .iter()
            .filter(|t| tag_attrs(**t).contains(&name))
            .map(|t| tag_ref(*t))
            .collect();
        if !owners.is_empty() {
            return Err(self.attr_err(
                a,
                format!("`{name}` is not an attribute of {}", tag_ref(tag)),
                format!("`{name}` is for {}", owners.join(", ")),
            ));
        }
        let mut candidates: Vec<&str> = GLOBAL_ATTRS.to_vec();
        candidates.extend_from_slice(tag_attrs(tag));
        let mut help = name_help(name, &candidates);
        if name == "data-" {
            help = "write a name after `data-`, like data-slot=\"2\"".to_owned();
        }
        Err(self.attr_err(
            a,
            format!("unknown attribute `{name}` on {}", tag_ref(tag)),
            help,
        ))
    }

    fn missing_value(&self, at: (u32, u32), name: &str) -> MarkupError {
        self.err(
            at,
            format!("`{name}=` has no value"),
            format!("write {name}=\"value\""),
        )
    }

    fn only(
        &self,
        tag: Tag,
        have: InputType,
        a: &RawAttr,
        wanted: InputType,
        spelled: &str,
    ) -> Result<(), MarkupError> {
        if tag == Tag::Input && have != wanted {
            return Err(self.attr_err(
                a,
                format!("`{}` only works on `<input type=\"{spelled}\">`", a.name),
                format!(
                    "add type=\"{spelled}\" to this input, or remove `{}`",
                    a.name
                ),
            ));
        }
        Ok(())
    }

    fn value<'b>(&self, a: &'b RawAttr) -> Result<&'b str, MarkupError> {
        a.value.as_deref().ok_or_else(|| {
            self.attr_err(
                a,
                format!("`{}` needs a value", a.name),
                format!("write {}=\"...\"", a.name),
            )
        })
    }

    fn flag(&self, a: &RawAttr) -> Result<bool, MarkupError> {
        match a.value.as_deref() {
            None | Some("") => Ok(true),
            Some(v) if v == a.name => Ok(true),
            Some(v) => Err(self.attr_err(
                a,
                format!(
                    "`{}` is on or off and takes no value, found \"{v}\"",
                    a.name
                ),
                format!("write just `{}` to turn it on, or leave it out", a.name),
            )),
        }
    }

    fn number(&self, a: &RawAttr) -> Result<f32, MarkupError> {
        let v = self.value(a)?;
        v.trim()
            .parse::<f32>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or_else(|| {
                self.attr_err(
                    a,
                    format!("`{}` must be a number, found \"{v}\"", a.name),
                    format!("write a number, like {}=\"10\"", a.name),
                )
            })
    }

    fn build_element(
        &mut self,
        tag: Tag,
        attrs: &[RawAttr],
        at: (u32, u32),
    ) -> Result<ElementId, MarkupError> {
        let el = self.doc.create_element(tag, at.0, at.1);
        match self.fill(el, tag, attrs, at) {
            Ok(()) => Ok(el),
            Err(e) => {
                self.doc.remove(el);
                Err(e)
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn fill(
        &mut self,
        el: ElementId,
        tag: Tag,
        attrs: &[RawAttr],
        at: (u32, u32),
    ) -> Result<(), MarkupError> {
        for (n, a) in attrs.iter().enumerate() {
            self.check_allowed(tag, a)?;
            if let Some(first) = attrs.iter().take(n).find(|f| f.name == a.name) {
                return Err(self.attr_err(
                    a,
                    format!("`{}` is written twice on this tag", a.name),
                    format!(
                        "remove one; the first is at line {}, column {}",
                        first.line, first.col
                    ),
                ));
            }
        }
        let find = |name: &str| attrs.iter().find(|a| a.name == name);

        let mut form = FormState::default();
        if tag == Tag::Input
            && let Some(a) = find("type")
        {
            form.input_type = match self.value(a)? {
                "text" => InputType::Text,
                "range" => InputType::Range,
                "checkbox" => InputType::Checkbox,
                other => {
                    return Err(self.attr_err(
                        a,
                        format!("unknown input type \"{other}\""),
                        name_help(other, &["text", "range", "checkbox"]),
                    ));
                }
            };
        }
        let input_type = form.input_type;

        let mut stored: Vec<(crate::intern::Atom, String)> = Vec::with_capacity(attrs.len());
        let mut classes = Vec::new();
        let mut handlers = [None; 4];
        let (mut hidden, mut disabled, mut autofocus) = (false, false, false);
        let mut tabindex = None;
        let mut style_attr = None;
        let mut value_attr: Option<f32> = None;
        let (mut min, mut max, mut step) = (None, None, None);
        let mut node_id = None;

        for a in attrs {
            let name = a.name.as_str();
            let text = if BOOLEAN_ATTRS.contains(&name) {
                self.flag(a)?;
                String::new()
            } else {
                self.value(a)?.to_owned()
            };
            match name {
                "id" => {
                    if text.is_empty() || text.chars().any(is_ws) {
                        return Err(self.attr_err(
                            a,
                            "an id must not be empty or contain spaces",
                            "write a single word, like id=\"play-button\"",
                        ));
                    }
                    let atom = self.doc.intern(&text);
                    if let Some(other) = self.doc.find_by_id(atom).filter(|o| *o != el) {
                        let (l, c) = self.doc.get(other).map_or((0, 0), |n| (n.line, n.column));
                        return Err(self.attr_err(
                            a,
                            format!("the id \"{text}\" is already used at line {l}, column {c}"),
                            "every id must be unique; pick another",
                        ));
                    }
                    node_id = Some(atom);
                }
                "class" => {
                    for word in text.split(is_ws).filter(|w| !w.is_empty()) {
                        classes.push(self.doc.intern(word));
                    }
                }
                "style" => {
                    style_attr = Some(SourceText {
                        text: text.clone(),
                        line: a.vline,
                        column: a.vcol,
                    });
                }
                "hidden" => hidden = true,
                "disabled" => disabled = true,
                "autofocus" => autofocus = true,
                "tabindex" => {
                    tabindex = Some(text.trim().parse::<i32>().map_err(|_| {
                        self.attr_err(
                            a,
                            format!("`tabindex` must be a whole number, found \"{text}\""),
                            "write tabindex=\"0\" to make it reachable, or \"-1\" to skip it",
                        )
                    })?);
                }
                "onclick" | "onchange" | "oninput" | "onfocus" => {
                    if !is_identifier(&text) {
                        return Err(self.attr_err(
                            a,
                            format!("`{name}` must name a function, found \"{text}\""),
                            format!("write the name of a function, like {name}=\"startGame\""),
                        ));
                    }
                    let kind = match name {
                        "onclick" => HandlerKind::Click,
                        "onchange" => HandlerKind::Change,
                        "oninput" => HandlerKind::Input,
                        _ => HandlerKind::Focus,
                    };
                    handlers[kind.index()] = Some(self.doc.intern(&text));
                }
                "src" if text.is_empty() => {
                    return Err(self.attr_err(
                        a,
                        "`src` is empty",
                        "write the picture's name, like src=\"logo\"",
                    ));
                }
                "href" if text.is_empty() => {
                    return Err(self.attr_err(
                        a,
                        "`href` is empty",
                        "write the file, like href=\"menu.css\"",
                    ));
                }
                "rel" if text != "stylesheet" => {
                    return Err(self.attr_err(
                        a,
                        format!("`rel` must be \"stylesheet\", found \"{text}\""),
                        "only style sheets can be linked: <link rel=\"stylesheet\" href=\"menu.css\">",
                    ));
                }
                "value" => match (tag, input_type) {
                    (Tag::Progress, _) | (Tag::Input, InputType::Range) => {
                        value_attr = Some(self.number(a)?);
                    }
                    _ => text.clone_into(&mut form.value),
                },
                "checked" => {
                    self.only(tag, input_type, a, InputType::Checkbox, "checkbox")?;
                    form.checked = true;
                }
                "selected" => form.checked = true,
                "placeholder" => self.only(tag, input_type, a, InputType::Text, "text")?,
                "maxlength" => {
                    self.only(tag, input_type, a, InputType::Text, "text")?;
                    form.max_length = Some(text.trim().parse::<u32>().map_err(|_| {
                        self.attr_err(
                            a,
                            format!("`maxlength` must be a whole number, found \"{text}\""),
                            "write a count, like maxlength=\"12\"",
                        )
                    })?);
                }
                "min" => {
                    self.only(tag, input_type, a, InputType::Range, "range")?;
                    min = Some(self.number(a)?);
                }
                "max" => {
                    if tag == Tag::Input {
                        self.only(tag, input_type, a, InputType::Range, "range")?;
                    }
                    max = Some(self.number(a)?);
                }
                "step" => {
                    self.only(tag, input_type, a, InputType::Range, "range")?;
                    let s = self.number(a)?;
                    if s <= 0.0 {
                        return Err(self.attr_err(
                            a,
                            format!("`step` must be more than 0, found \"{text}\""),
                            "write a step like step=\"1\" or step=\"0.1\"",
                        ));
                    }
                    step = Some(s);
                }
                _ => {}
            }
            let atom = self.doc.intern(name);
            stored.push((atom, text));
        }

        match tag {
            Tag::Img if find("src").is_none() => {
                return Err(self.err(
                    at,
                    "`<img>` needs a `src`",
                    "write the picture's name, like <img src=\"logo\">",
                ));
            }
            Tag::Link => {
                if find("rel").is_none() {
                    return Err(self.err(
                        at,
                        "`<link>` needs `rel=\"stylesheet\"`",
                        "write <link rel=\"stylesheet\" href=\"menu.css\">",
                    ));
                }
                if find("href").is_none() {
                    return Err(self.err(
                        at,
                        "`<link>` needs an `href`",
                        "write <link rel=\"stylesheet\" href=\"menu.css\">",
                    ));
                }
            }
            _ => {}
        }

        match (tag, input_type) {
            (Tag::Input, InputType::Range) => {
                let lo = min.unwrap_or(0.0);
                let hi = max.unwrap_or(100.0);
                let st = step.unwrap_or(1.0);
                if hi < lo {
                    return Err(self.err(
                        at,
                        format!("`max` ({hi}) is smaller than `min` ({lo})"),
                        "make `max` at least `min`",
                    ));
                }
                let raw = value_attr.unwrap_or(lo + (hi - lo) / 2.0);
                let snapped = lo + ((raw - lo) / st).round() * st;
                form.min = lo;
                form.max = hi;
                form.step = st;
                form.number = snapped.clamp(lo, hi);
            }
            (Tag::Input, InputType::Checkbox) => {
                if find("value").is_none() {
                    "on".clone_into(&mut form.value);
                }
            }
            (Tag::Progress, _) => {
                let hi = max.unwrap_or(1.0);
                if hi <= 0.0 {
                    return Err(self.err(
                        at,
                        format!("`max` must be more than 0, found {hi}"),
                        "write max=\"100\" or leave it out for 1",
                    ));
                }
                form.min = 0.0;
                form.max = hi;
                form.number = value_attr.unwrap_or(0.0).clamp(0.0, hi);
            }
            _ => {}
        }

        if let Some(atom) = node_id {
            self.doc.set_id(el, Some(atom));
        }
        if let Some(n) = self.doc.get_mut(el) {
            n.attrs = stored;
            n.classes = classes;
            n.handlers = handlers;
            n.hidden = hidden;
            n.disabled = disabled;
            n.autofocus = autofocus;
            n.tabindex = tabindex;
            n.style_attr = style_attr;
            n.form = form;
        }
        Ok(())
    }
}
