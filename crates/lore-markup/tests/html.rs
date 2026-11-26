//! Tests of the HTML parser through the public API.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use lore_markup::{
    Document, ElementId, HandlerKind, InputType, MarkupError, Parsed, Tag, parse, parse_fragment,
};

fn ok(src: &str) -> Parsed {
    match parse("t.html", src) {
        Ok(p) => p,
        Err(e) => panic!("expected Ok for {src:?}, got:\n{e}"),
    }
}

fn err(src: &str) -> MarkupError {
    match parse("t.html", src) {
        Ok(_) => panic!("expected an error for {src:?}"),
        Err(e) => e,
    }
}

/// Asserts the error position and returns it for message checks.
fn err_at(src: &str, line: u32, col: u32) -> MarkupError {
    let e = err(src);
    assert_eq!(
        (e.line, e.column),
        (line, col),
        "wrong position for {src:?}: {e}"
    );
    assert_eq!(e.file, "t.html");
    assert!(!e.help.is_empty(), "help must not be empty: {e}");
    e
}

fn kids(doc: &Document, id: ElementId) -> Vec<String> {
    doc.children(id)
        .map(|c| {
            let n = doc.get(c).unwrap();
            match n.tag() {
                Some(t) => t.name().to_owned(),
                None => format!("#{}", n.text().unwrap()),
            }
        })
        .collect()
}

fn by_id(doc: &Document, id: &str) -> ElementId {
    doc.find_by_id_str(id).unwrap()
}

const FULL: &str = r#"<!DOCTYPE html>
<html>
<head>
  <title>Menu</title>
  <link rel="stylesheet" href="menu.css">
  <style>p { color: red }</style>
</head>
<body>
  <div id="a" class="x y"><h1>T</h1><h2>S</h2><h3>U</h3><p>P <span>s</span> <label for="n">L</label><br></p>
  <img src="logo" alt="Logo">
  <button>b</button>
  <input name="n">
  <select name="s"><option value="1">One</option><option>Two</option></select>
  <progress value="1" max="2"></progress>
  </div>
</body>
</html>
"#;

#[test]
fn every_tag_is_accepted() {
    let p = ok(FULL);
    let doc = &p.document;
    let mut seen: Vec<Tag> = doc
        .descendants(doc.root())
        .filter_map(|d| doc.get(d).and_then(|n| n.tag()))
        .collect();
    seen.dedup();
    for t in Tag::ALL {
        assert!(seen.contains(&t), "missing tag {}", t.name());
    }
    assert_eq!(kids(doc, doc.root()), ["html"]);
    let html = doc.first_child(doc.root()).unwrap();
    assert_eq!(kids(doc, html), ["head", "body"]);
}

#[test]
fn nesting_and_text() {
    let p = ok(FULL);
    let doc = &p.document;
    let a = by_id(doc, "a");
    assert_eq!(
        kids(doc, a),
        [
            "h1", "h2", "h3", "p", "img", "button", "input", "select", "progress"
        ]
    );
    let para = doc.children(a).nth(3).unwrap();
    assert_eq!(kids(doc, para), ["#P ", "span", "# ", "label", "br"]);
    assert_eq!(doc.parent(para), Some(a));
    assert_eq!(doc.text_content(a), "TSUP s LbOneTwo");
}

#[test]
fn style_and_link_are_reported() {
    let p = ok(FULL);
    assert_eq!(p.styles.len(), 1);
    assert_eq!(p.styles[0].text, "p { color: red }");
    assert_eq!((p.styles[0].line, p.styles[0].column), (6, 10));
    assert_eq!(p.links.len(), 1);
    assert_eq!(p.links[0].href, "menu.css");
    assert_eq!((p.links[0].line, p.links[0].column), (5, 3));
}

#[test]
fn style_block_text_is_raw_and_positioned() {
    let p = ok("<style>\n  a > b { x: \"<i>\" }\n</style>\n<style></style>");
    assert_eq!(p.styles.len(), 2);
    assert_eq!(p.styles[0].text, "\n  a > b { x: \"<i>\" }\n");
    assert_eq!((p.styles[0].line, p.styles[0].column), (1, 8));
    assert_eq!(p.styles[1].text, "");
    assert_eq!((p.styles[1].line, p.styles[1].column), (4, 8));
    assert!(p.document.contains(p.styles[0].element));
}

#[test]
fn title_text_is_ignored() {
    let p = ok("<html><head><title>a < b &bogus;</title></head><body></body></html>");
    let doc = &p.document;
    let head = doc
        .children(doc.first_child(doc.root()).unwrap())
        .next()
        .unwrap();
    let title = doc.first_child(head).unwrap();
    assert_eq!(doc.get(title).unwrap().tag(), Some(Tag::Title));
    assert_eq!(doc.first_child(title), None);
}

#[test]
fn void_tags_with_and_without_slash() {
    let p = ok(r#"<div><br><br/><br /><img src="a"><img src="b"/><input><input/></div>"#);
    let d = &p.document;
    let div = d.first_child(d.root()).unwrap();
    assert_eq!(
        kids(d, div),
        ["br", "br", "br", "img", "img", "input", "input"]
    );
}

#[test]
fn comments_are_dropped_and_do_not_split_text() {
    let p = ok("<p>a<!-- hidden <b> -->b</p><!-- x -->");
    let d = &p.document;
    let para = d.first_child(d.root()).unwrap();
    assert_eq!(kids(d, para), ["#ab"]);
}

#[test]
fn entities_are_decoded() {
    let p = ok("<p>a &amp; b &lt;&gt; &quot;&apos; &nbsp;&#65;&#x42;&#x1F600;</p>");
    let d = &p.document;
    assert_eq!(d.text_content(d.root()), "a & b <> \"' \u{a0}AB\u{1F600}");
}

#[test]
fn entities_in_attribute_values() {
    let p = ok(r#"<img id="a" src="x" alt="Tom &amp; Jerry &#x21;">"#);
    let d = &p.document;
    let n = d.get(by_id(d, "a")).unwrap();
    assert_eq!(n.attr(d.interner(), "alt"), Some("Tom & Jerry !"));
}

#[test]
fn whitespace_collapses() {
    let p = ok("<p>  a \n\t b  </p>");
    let d = &p.document;
    assert_eq!(kids(d, d.first_child(d.root()).unwrap()), ["# a b "]);
}

#[test]
fn blank_text_between_blocks_is_dropped_but_kept_between_inline() {
    let p = ok("<div>\n  <p>a</p>\n  <p>b</p>\n</div><div><span>a</span> <span>b</span> </div>");
    let d = &p.document;
    let mut roots = d.children(d.root());
    let first = roots.next().unwrap();
    let second = roots.next().unwrap();
    assert_eq!(kids(d, first), ["p", "p"]);
    assert_eq!(kids(d, second), ["span", "# ", "span"]);
}

#[test]
fn blank_text_next_to_br_and_buttons_is_dropped() {
    let p = ok("<div>a <br> <button>x</button> </div>");
    let d = &p.document;
    assert_eq!(
        kids(d, d.first_child(d.root()).unwrap()),
        ["#a ", "br", "button"]
    );
}

#[test]
fn global_attributes_are_stored() {
    let p = ok(
        r#"<button id="go" class=" big  red " style="color: red" hidden disabled tabindex="-1" autofocus
            data-slot="2" onclick="start" onchange="changed" oninput="typing" onfocus="focused">x</button>"#,
    );
    let d = &p.document;
    let b = d.get(by_id(d, "go")).unwrap();
    let names: Vec<&str> = b.classes.iter().map(|c| d.resolve(*c)).collect();
    assert_eq!(names, ["big", "red"]);
    assert!(b.hidden && b.disabled && b.autofocus);
    assert_eq!(b.tabindex, Some(-1));
    assert_eq!(b.attr(d.interner(), "data-slot"), Some("2"));
    let h = |k: HandlerKind| b.handlers[k.index()].map(|a| d.resolve(a));
    assert_eq!(h(HandlerKind::Click), Some("start"));
    assert_eq!(h(HandlerKind::Change), Some("changed"));
    assert_eq!(h(HandlerKind::Input), Some("typing"));
    assert_eq!(h(HandlerKind::Focus), Some("focused"));
    let s = b.style_attr.as_ref().unwrap();
    assert_eq!(s.text, "color: red");
    assert_eq!((s.line, s.column), (1, 43));
    assert_eq!((b.line, b.column), (1, 1));
}

#[test]
fn quoting_styles_and_boolean_forms() {
    let p = ok(
        r#"<div id=a class='b c' hidden="" disabled="disabled" data-x=y/>"#
            .replace("y/>", "y></div>")
            .as_str(),
    );
    let d = &p.document;
    let n = d.get(by_id(d, "a")).unwrap();
    assert_eq!(n.classes.len(), 2);
    assert!(n.hidden && n.disabled);
    assert_eq!(n.attr(d.interner(), "data-x"), Some("y"));
}

#[test]
fn unquoted_value_before_self_close() {
    let p = ok("<img src=logo/>");
    let d = &p.document;
    let img = d.first_child(d.root()).unwrap();
    assert_eq!(d.get(img).unwrap().attr(d.interner(), "src"), Some("logo"));
}

#[test]
fn per_tag_attributes_are_accepted() {
    ok(r#"<img src="a" alt="b">"#);
    ok(r#"<input type="text" name="n" value="v" placeholder="p" maxlength="5">"#);
    ok(r#"<input type="range" name="n" value="5" min="0" max="10" step="1">"#);
    ok(r#"<input type="checkbox" name="n" value="v" checked>"#);
    ok(r#"<select name="s"><option value="1" selected>a</option></select>"#);
    ok(r#"<label for="x">a</label>"#);
    ok(r#"<progress value="1" max="2"></progress>"#);
    ok(r#"<link rel="stylesheet" href="a.css">"#);
}

#[test]
fn text_input_state() {
    let p = ok(r#"<input id="a" value="hi" maxlength="8" placeholder="name">"#);
    let d = &p.document;
    let f = &d.get(by_id(d, "a")).unwrap().form;
    assert_eq!(f.value, "hi");
    assert_eq!(f.max_length, Some(8));
    assert_eq!(f.input_type, InputType::Text);
}

#[test]
fn checkbox_state() {
    let p = ok(r#"<input id="a" type="checkbox" checked><input id="b" type="checkbox">"#);
    let d = &p.document;
    let a = &d.get(by_id(d, "a")).unwrap().form;
    assert!(a.checked);
    assert_eq!(a.input_type, InputType::Checkbox);
    assert!(!d.get(by_id(d, "b")).unwrap().form.checked);
}

#[test]
fn range_state_is_snapped_and_clamped() {
    let form = |attrs: &str| {
        let p = ok(&format!(r#"<input id="r" type="range" {attrs}>"#));
        let d = &p.document;
        d.get(by_id(d, "r")).unwrap().form.clone()
    };
    let f = form(r#"min="0" max="10" step="2" value="7""#);
    assert_eq!((f.min, f.max, f.step, f.number), (0.0, 10.0, 2.0, 8.0));
    assert_eq!(form(r#"min="0" max="10" value="50""#).number, 10.0);
    assert_eq!(form(r#"min="0" max="10" value="-5""#).number, 0.0);
    let d = form("");
    assert_eq!((d.min, d.max, d.step, d.number), (0.0, 100.0, 1.0, 50.0));
    assert_eq!(
        form(r#"min="-1" max="1" step="0.5" value="0.4""#).number,
        0.5
    );
    assert_eq!(form(r#"min="3" max="3""#).number, 3.0);
}

#[test]
fn progress_state() {
    let form = |attrs: &str| {
        let p = ok(&format!(r#"<progress id="r" {attrs}></progress>"#));
        let d = &p.document;
        d.get(by_id(d, "r")).unwrap().form.clone()
    };
    let f = form(r#"value="5" max="10""#);
    assert_eq!((f.number, f.min, f.max), (5.0, 0.0, 10.0));
    assert_eq!(form(r#"value="20" max="10""#).number, 10.0);
    assert_eq!(form(r#"value="-1" max="10""#).number, 0.0);
    let f = form("");
    assert_eq!((f.number, f.max), (0.0, 1.0));
}

#[test]
fn select_state() {
    let p = ok(
        r#"<select id="s"><option value="a">A</option><option selected>B</option><option>C</option></select>
           <select id="t"><option>X</option><option>Y</option></select>
           <select id="e"></select>"#,
    );
    let d = &p.document;
    let s = &d.get(by_id(d, "s")).unwrap().form;
    assert_eq!((s.selected, s.value.as_str()), (1, "B"));
    let t = &d.get(by_id(d, "t")).unwrap().form;
    assert_eq!((t.selected, t.value.as_str()), (0, "X"));
    let e = &d.get(by_id(d, "e")).unwrap().form;
    assert_eq!((e.selected, e.value.as_str()), (0, ""));
}

#[test]
fn doctype_is_optional_and_case_blind() {
    ok("<!doctype html><p>a</p>");
    ok("<!DOCTYPE html>\n\n<p>a</p>");
    ok("<!-- c --><!DOCTYPE html><p>a</p>");
}

#[test]
fn positions_follow_lines_and_columns() {
    let p = ok("<div>\n  <p id=\"x\">a</p>\n    <span id=\"y\">b</span>\n</div>");
    let d = &p.document;
    let x = d.get(by_id(d, "x")).unwrap();
    assert_eq!((x.line, x.column), (2, 3));
    let y = d.get(by_id(d, "y")).unwrap();
    assert_eq!((y.line, y.column), (3, 5));
}

// ----- errors -----

#[test]
fn unknown_tag_suggests() {
    let e = err_at("<div>\n  <buton>x</buton>\n</div>", 2, 4);
    assert_eq!(e.message, "unknown tag `buton`");
    assert_eq!(e.help, "did you mean `button`?");
}

#[test]
fn unknown_tag_without_near_match_lists_tags() {
    let e = err_at("<marquee>", 1, 2);
    assert!(e.help.starts_with("use one of: html, head"), "{}", e.help);
}

#[test]
fn uppercase_tag_says_lowercase() {
    let e = err_at("<DIV></DIV>", 1, 2);
    assert_eq!(e.help, "write `div` in lowercase");
}

#[test]
fn unknown_close_tag() {
    let e = err_at("<div></dvi>", 1, 8);
    assert_eq!(e.message, "unknown tag `dvi`");
    assert_eq!(e.help, "did you mean `div`?");
}

#[test]
fn unknown_attribute_suggests() {
    let e = err_at(r#"<div clas="a"></div>"#, 1, 6);
    assert!(e.message.contains("unknown attribute `clas`"), "{e}");
    assert_eq!(e.help, "did you mean `class`?");
}

#[test]
fn handler_names_are_suggested_case_blind() {
    let e = err_at(r#"<button onClick="go">x</button>"#, 1, 9);
    assert_eq!(e.help, "did you mean `onclick`?");
}

#[test]
fn attribute_for_the_wrong_tag() {
    let e = err_at(r#"<div src="a"></div>"#, 1, 6);
    assert!(
        e.message.contains("`src` is not an attribute of `<div>`"),
        "{e}"
    );
    assert!(e.help.contains("`<img>`"), "{e}");
    let e = err_at(r#"<input type="range" for="x">"#, 1, 21);
    assert!(e.help.contains("`<label>`"), "{e}");
}

#[test]
fn input_attributes_need_the_right_type() {
    let e = err_at(r#"<input type="range" placeholder="x">"#, 1, 21);
    assert!(
        e.message.contains("only works on `<input type=\"text\">`"),
        "{e}"
    );
    let e = err_at(r#"<input min="1">"#, 1, 8);
    assert!(e.help.contains("type=\"range\""), "{e}");
    err(r#"<input checked>"#);
    err(r#"<input type="text" step="1">"#);
}

#[test]
fn duplicate_attribute() {
    let e = err_at(r#"<div class="a" class="b"></div>"#, 1, 16);
    assert!(e.message.contains("`class` is written twice"), "{e}");
    assert!(e.help.contains("line 1, column 6"), "{e}");
}

#[test]
fn duplicate_id_points_at_both() {
    let e = err_at("<div id=\"a\"></div>\n<div id=\"a\"></div>", 2, 6);
    assert!(
        e.message.contains("already used at line 1, column 1"),
        "{e}"
    );
}

#[test]
fn id_must_be_one_word() {
    err_at(r#"<div id=""></div>"#, 1, 6);
    err_at(r#"<div id="a b"></div>"#, 1, 6);
}

#[test]
fn bad_handler_names() {
    let e = err_at(r#"<button onclick="a b">x</button>"#, 1, 9);
    assert!(e.help.contains("onclick=\"startGame\""), "{e}");
    err_at(r#"<button onclick="1go">x</button>"#, 1, 9);
    err_at(r#"<button onclick="">x</button>"#, 1, 9);
    err_at(r#"<button onclick="a.b">x</button>"#, 1, 9);
    err_at(r#"<button onclick>x</button>"#, 1, 9);
    ok(r#"<button onclick="_a1">x</button>"#);
}

#[test]
fn unclosed_tag_points_at_the_open_tag() {
    let e = err_at("<div>\n<p>text", 2, 1);
    assert_eq!(e.message, "`<p>` is never closed");
    assert_eq!(e.help, "add `</p>` before the end of the file");
}

#[test]
fn mismatched_close_names_the_open_tag() {
    let e = err_at("<div><span></div>", 1, 12);
    assert_eq!(
        e.message,
        "`</div>` found, but `<span>` opened at line 1, column 6 is still open"
    );
    assert_eq!(e.help, "close `<span>` first with `</span>`");
    let e = err_at("<div>\n  <p>a\n</div>", 3, 1);
    assert!(
        e.message.contains("`<p>` opened at line 2, column 3"),
        "{e}"
    );
}

#[test]
fn stray_close_tag() {
    let e = err_at("</div>", 1, 1);
    assert!(e.message.contains("never opened"), "{e}");
    err_at("<p>a</p>\n</p>", 2, 1);
}

#[test]
fn void_tags_cannot_be_closed() {
    let e = err_at("<p>a</br></p>", 1, 5);
    assert!(e.message.contains("has no closing tag"), "{e}");
}

#[test]
fn only_void_tags_close_themselves() {
    let e = err_at("<div/>", 1, 1);
    assert!(e.help.contains("<div></div>"), "{e}");
}

#[test]
fn bad_entities() {
    let e = err_at("<p>a &foo; b</p>", 1, 6);
    assert!(e.message.contains("unknown entity `&foo;`"), "{e}");
    let e = err_at("<p>a &amp b</p>", 1, 6);
    assert!(e.message.contains("missing its `;`"), "{e}");
    let e = err_at("<p>a & b</p>", 1, 6);
    assert_eq!(e.message, "a bare `&`");
    err_at("<p>&#0;</p>", 1, 4);
    err_at("<p>&#xD800;</p>", 1, 4);
    err_at("<p>&#xZZ;</p>", 1, 4);
    err_at("<p>&#99999999999;</p>", 1, 4);
    let e = err_at("<p>&amq;</p>", 1, 4);
    assert_eq!(e.help, "did you mean `amp`?");
    err_at(r#"<div id="a&b"></div>"#, 1, 11);
}

#[test]
fn unterminated_comment() {
    let e = err_at("<p></p>\n<!-- x", 2, 1);
    assert_eq!(e.message, "this comment is never closed");
}

#[test]
fn unterminated_attribute_value() {
    let e = err_at("<div class=\"abc", 1, 12);
    assert!(e.message.contains("never closed"), "{e}");
    err_at("<div class='abc\n\n", 1, 12);
}

#[test]
fn unterminated_tag_at_eof() {
    let e = err_at("<p></p>\n<div class=\"a\"", 2, 1);
    assert!(e.message.contains("`<div` is never finished"), "{e}");
    err_at("<div", 1, 1);
    err_at("<div ", 1, 1);
    err_at("<div class", 1, 1);
    err_at("<div class=", 1, 1);
    err_at("<div class=a", 1, 1);
    err_at("<p></p", 1, 4);
    err_at("<", 1, 1);
}

#[test]
fn unterminated_style_and_declaration() {
    let e = err_at("<style>a {}", 1, 1);
    assert!(e.message.contains("`<style>` is never closed"), "{e}");
    err_at("<!DOCTYPE html", 1, 1);
}

#[test]
fn img_needs_src() {
    let e = err_at("<div>\n  <img alt=\"x\">\n</div>", 2, 3);
    assert_eq!(e.message, "`<img>` needs a `src`");
    err_at(r#"<img src="">"#, 1, 6);
    err_at("<img src>", 1, 6);
}

#[test]
fn bad_input_type() {
    let e = err_at(r#"<input type="radio">"#, 1, 8);
    assert!(e.message.contains("unknown input type \"radio\""), "{e}");
    assert!(e.help.contains("text, range, checkbox"), "{e}");
    let e = err_at(r#"<input type="chekbox">"#, 1, 8);
    assert_eq!(e.help, "did you mean `checkbox`?");
}

#[test]
fn bad_numbers() {
    err_at(r#"<input type="range" min="abc">"#, 1, 21);
    err_at(r#"<input type="range" step="0">"#, 1, 21);
    err_at(r#"<input type="range" min="5" max="1">"#, 1, 1);
    err_at(r#"<progress value="x"></progress>"#, 1, 11);
    err_at(r#"<progress max="0"></progress>"#, 1, 1);
    err_at(r#"<input maxlength="-1">"#, 1, 8);
    err_at(r#"<div tabindex="a"></div>"#, 1, 6);
    err_at(r#"<input type="range" value="NaN">"#, 1, 21);
}

#[test]
fn boolean_attributes_take_no_value() {
    let e = err_at(r#"<div hidden="no"></div>"#, 1, 6);
    assert!(e.help.contains("write just `hidden`"), "{e}");
}

#[test]
fn link_rules() {
    err_at(r#"<link href="a.css">"#, 1, 1);
    err_at(r#"<link rel="stylesheet">"#, 1, 1);
    err_at(r#"<link rel="icon" href="a">"#, 1, 7);
}

#[test]
fn structure_rules() {
    err_at("<html><html></html></html>", 1, 7);
    err_at("<div><head></head></div>", 1, 6);
    err_at("<html><div></div></html>", 1, 7);
    err_at("<html><head><div></div></head></html>", 1, 13);
    err_at("<html><body></body></html><div></div>", 1, 27);
    err_at("<div><title>a</title></div>", 1, 6);
    err_at("<select><div></div></select>", 1, 9);
    err_at("<option>a</option>", 1, 1);
    err_at("<select><option><b>x</b></option></select>", 1, 18);
    err_at("<html>hello</html>", 1, 7);
    err_at("<select>hi</select>", 1, 9);
    err_at("<head></head>", 1, 1);
    err_at("<html><head></head><head></head></html>", 1, 20);
}

#[test]
fn doctype_rules() {
    err_at("<p></p><!DOCTYPE html>", 1, 8);
    err_at("<!DOCTYPE html><!DOCTYPE html>", 1, 16);
    err_at("<!DOCTYPE svg>", 1, 1);
    err_at("<![CDATA[x]]>", 1, 1);
}

#[test]
fn stray_syntax_inside_tags() {
    err_at("<div / >", 1, 6);
    err_at("<div \"x\">", 1, 6);
    err_at("<div class=\"a\"\"b\">", 1, 15);
    err_at("<div class=a\"b>", 1, 13);
    err_at("<div class=>", 1, 11);
    err_at("<div class= >", 1, 11);
    err_at("<p>a < b</p>", 1, 6);
    err_at("<div data->", 1, 6);
    err_at("<div 1a=\"x\"></div>", 1, 6);
    err_at("<p></ p>", 1, 4);
    err_at("<p></p x>", 1, 8);
}

#[test]
fn error_display_format() {
    let e = err("<div>\n  <buton></buton></div>");
    assert_eq!(
        e.to_string(),
        "t.html:2:4: unknown tag `buton`\n  help: did you mean `button`?"
    );
}

#[test]
fn errors_count_columns_in_chars() {
    // Two multi-byte characters before the bad tag.
    let e = err_at("<p>\u{e9}\u{1F600}</p><buton></buton>", 1, 11);
    assert_eq!(e.message, "unknown tag `buton`");
}

// ----- fragments -----

#[test]
fn fragments_append_and_roll_back() {
    let mut p = ok(r#"<div id="list"><p id="one">1</p></div>"#);
    let doc = &mut p.document;
    let list = by_id(doc, "list");
    let added = parse_fragment(doc, list, "frag", r#"<p id="two">2</p> <span>x</span>"#).unwrap();
    assert_eq!(added.len(), 2);
    assert_eq!(kids(doc, list), ["p", "p", "span"]);
    assert!(doc.find_by_id_str("two").is_some());

    let before = doc.len();
    let e =
        parse_fragment(doc, list, "frag", r#"<p id="three">3</p><p id="one"></p>"#).unwrap_err();
    assert!(e.message.contains("already used"), "{e}");
    assert_eq!(e.file, "frag");
    assert_eq!(doc.len(), before);
    assert_eq!(doc.find_by_id_str("three"), None);
    assert_eq!(kids(doc, list), ["p", "p", "span"]);

    let e = parse_fragment(doc, list, "frag", "<div><p>").unwrap_err();
    assert!(e.message.contains("never closed"));
    assert_eq!(doc.len(), before);
}

#[test]
fn fragments_refuse_page_tags_and_dead_targets() {
    let mut p = ok(r#"<div id="a">text</div>"#);
    let doc = &mut p.document;
    let a = by_id(doc, "a");
    assert!(parse_fragment(doc, a, "f", "<body></body>").is_err());
    assert!(parse_fragment(doc, a, "f", "<style></style>").is_err());
    let text = doc.first_child(a).unwrap();
    assert!(parse_fragment(doc, text, "f", "<p></p>").is_err());
    assert!(doc.remove(a));
    assert!(parse_fragment(doc, a, "f", "<p></p>").is_err());
}

#[test]
fn fragment_into_select_refreshes_choice() {
    let mut p = ok(r#"<select id="s"><option>A</option></select>"#);
    let doc = &mut p.document;
    let s = by_id(doc, "s");
    parse_fragment(doc, s, "f", "<option selected>B</option>").unwrap();
    let f = &doc.get(s).unwrap().form;
    assert_eq!((f.selected, f.value.as_str()), (1, "B"));
    assert!(parse_fragment(doc, s, "f", "<p></p>").is_err());
}

// ----- never panics -----

const MENU: &str = r#"<!DOCTYPE html>
<html>
<head>
  <title>Pause &amp; options</title>
  <link rel="stylesheet" href="menu.css">
  <style>
    .menu { display: flex; gap: 8px }
    button:focus { outline: 3px solid #ffd75e }
  </style>
</head>
<body>
  <!-- the pause menu -->
  <div id="menu" class="menu big" data-page=main>
    <h1>Paused</h1>
    <p>Welcome back, <span class='name'>traveller</span>&nbsp;&#33;</p>
    <button id="resume" onclick="resume" autofocus>Resume</button>
    <button onclick="openOptions" tabindex=0>Options</button>
    <label for="vol">Volume</label>
    <input id="vol" type="range" min="0" max="10" step="1" value="7" onchange="setVolume">
    <input type="checkbox" name="subs" checked oninput="toggle">
    <input type="text" name="who" placeholder="Name" maxlength="12" value="Ana"><br/>
    <select name="q" onchange="setQuality"><option value="lo">Low</option><option selected>High</option></select>
    <progress value="3" max="10"></progress>
    <img src="logo" alt="Logo" style="width: 64px; height: 64px">
    <button disabled>Quit</button>
  </div>
</body>
</html>
"#;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Parses; if it works, walks the whole tree to prove it is well formed.
fn survive(src: &str) {
    if let Ok(p) = parse("fuzz.html", src) {
        let d = &p.document;
        let mut count = 0;
        for id in d.descendants(d.root()) {
            assert!(d.contains(id));
            let parent = d.parent(id).unwrap();
            assert!(d.children(parent).any(|c| c == id));
            count += 1;
        }
        assert_eq!(count + 1, d.len());
    }
}

#[test]
fn the_menu_itself_parses() {
    let p = ok(MENU);
    assert_eq!(p.styles.len(), 1);
    assert_eq!(p.links.len(), 1);
}

#[test]
fn every_truncation_returns_something() {
    let chars: Vec<char> = MENU.chars().collect();
    for n in 0..=chars.len() {
        let s: String = chars[..n].iter().collect();
        survive(&s);
    }
}

#[test]
fn random_byte_mutations_never_panic() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let spicy = b"<>&\"'=/!-; \n\t#x0aA</>&#;";
    let base = MENU.as_bytes();
    for _ in 0..4000 {
        let mut bytes = base.to_vec();
        for _ in 0..=rng.below(4) {
            if bytes.is_empty() {
                break;
            }
            let at = rng.below(bytes.len());
            match rng.below(4) {
                0 => bytes[at] = rng.next().to_le_bytes()[0],
                1 => bytes[at] = spicy[rng.below(spicy.len())],
                2 => {
                    bytes.remove(at);
                }
                _ => bytes.insert(at, spicy[rng.below(spicy.len())]),
            }
        }
        survive(&String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn random_fragments_never_panic_and_roll_back() {
    let mut rng = Rng(42);
    let base = MENU.as_bytes();
    let mut p = ok(r#"<div id="host"></div>"#);
    let host = by_id(&p.document, "host");
    for _ in 0..1500 {
        let a = rng.below(base.len());
        let len = rng.below(120);
        let end = (a + len).min(base.len());
        let piece = String::from_utf8_lossy(&base[a..end]).into_owned();
        let before = p.document.len();
        if parse_fragment(&mut p.document, host, "f", &piece).is_err() {
            assert_eq!(p.document.len(), before, "rollback failed for {piece:?}");
        }
        // Keep the host small.
        let kids: Vec<_> = p.document.children(host).collect();
        for k in kids {
            p.document.remove(k);
        }
    }
}

#[test]
fn nesting_deeper_than_256_is_an_error_with_a_position() {
    let deep = |n: usize| format!("{}x{}", "<div>".repeat(n), "</div>".repeat(n));
    assert!(parse("t.html", &deep(200)).is_ok());
    let e = parse("t.html", &deep(300)).unwrap_err();
    assert!(e.message.contains("nest more than 256 deep"), "{e}");
    assert!(!e.help.is_empty());
}

#[test]
fn fragment_nesting_counts_the_parents_depth() {
    let deep = |n: usize| format!("{}x{}", "<div>".repeat(n), "</div>".repeat(n));
    let mut p = parse(
        "t.html",
        &format!("<html><body>{}</body></html>", deep(200)),
    )
    .unwrap();
    let doc = &mut p.document;
    let mut inner = doc.root();
    while let Some(c) = doc.first_child(inner) {
        if doc.get(c).and_then(|n| n.tag()).is_none() {
            break;
        }
        inner = c;
    }
    let e = parse_fragment(doc, inner, "frag", &deep(100)).unwrap_err();
    assert!(e.message.contains("nest more than 256 deep"), "{e}");
}
