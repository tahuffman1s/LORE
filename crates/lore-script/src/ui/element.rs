//! What a script holds for one element of a screen's page, the proxies for
//! `element.style` and `screen.values`, and the table an event handler is
//! given.

use std::rc::Rc;

use lore_markup::{ElementId, EventValue, HandlerKind, MarkupError, Screen as Page, Tag};
use mlua::{AnyUserData, Lua, MetaMethod, Table, UserData, UserDataMethods, Value};

use super::{ELEMENT_FIELDS, ELEMENT_METHODS, ElementRef, Fired, Problem, ScreenId};
use crate::api::{Shared, describe, fail};
use crate::character_api::shared_of;
use crate::error::closest;

fn key_text(key: &Value) -> String {
    match key {
        Value::String(key) => key.to_string_lossy(),
        other => other.to_string().unwrap_or_default(),
    }
}

/// What to tell a script whose screen or element cannot be used.
pub(super) fn problem(lua: &Lua, problem: &Problem) -> mlua::Error {
    match problem {
        Problem::NoScreen => fail(
            lua,
            "this screen was removed, or the script was saved and no longer loads it. Load it \
             again with game.ui.load(\"ui/main.html\")",
        ),
        Problem::NoPage(file) => fail(
            lua,
            format!(
                "{file} has a mistake in it, so nothing was read from it and there is nothing \
                 to find on it yet. The mistake, with its line, is on the error panel: fix it \
                 and save the file."
            ),
        ),
        Problem::Stale(file) => fail(
            lua,
            format!(
                "this element is from before {file} was reloaded (or was removed); find it \
                 again with screen:find(...)"
            ),
        ),
    }
}

/// A mistake the page's editing calls found, with what to write instead.
pub(super) fn markup_fail(lua: &Lua, what: &str, error: &MarkupError) -> mlua::Error {
    fail(lua, format!("{what}: {}. {}", error.message, error.help))
}

/// A string a script gave, or the sentence that says what to give.
fn text_arg(lua: &Lua, value: &Value, function: &str, example: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        other => Err(fail(
            lua,
            format!(
                "{function} needs text, but it was given {}. Write it like this: {example}",
                describe(other)
            ),
        )),
    }
}

/// The element a method was called on.
fn this_of(lua: &Lua, this: &Value, method: &str) -> mlua::Result<ElementRef> {
    let found = match this {
        Value::UserData(this) => this.borrow::<ElementValue>().ok().map(|this| this.0),
        _ => None,
    };
    found.ok_or_else(|| {
        fail(
            lua,
            format!(
                "{method} needs a colon, not a dot, because it acts on one element. Write it \
                 like this: element:{method}(...)"
            ),
        )
    })
}

/// An element, as a script holds it.
#[derive(Clone, Copy)]
pub(crate) struct ElementValue(pub ElementRef);

/// `element.style`.
struct StyleProxy(ElementRef);

/// `screen.values`.
struct ValuesProxy(ScreenId);

pub(crate) fn element(lua: &Lua, element: ElementRef) -> mlua::Result<AnyUserData> {
    lua.create_userdata(ElementValue(element))
}

pub(crate) fn values(lua: &Lua, screen: ScreenId) -> mlua::Result<AnyUserData> {
    lua.create_userdata(ValuesProxy(screen))
}

/// The table an event handler is given.
pub(crate) fn event_table(lua: &Lua, fired: &Fired) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    table.set("name", fired.name.as_str())?;
    table.set("kind", fired.kind.word())?;
    if let Some((id, generation)) = fired.element {
        let element = ElementRef {
            screen: fired.screen,
            id,
            generation,
        };
        table.set("element", self::element(lua, element)?)?;
    }
    match &fired.value {
        EventValue::None => {}
        EventValue::Text(text) => table.set("value", text.as_str())?,
        EventValue::Number(number) => table.set("value", *number)?,
        EventValue::Bool(on) => table.set("value", *on)?,
    }
    table.set("screen", super::api::screen_value(lua, fired.screen)?)?;
    Ok(table)
}

/// The names of the inputs and drop-downs of a page.
pub(super) fn input_names(page: &Page) -> Vec<String> {
    let doc = page.document();
    doc.descendants(doc.root())
        .filter_map(|id| {
            let node = doc.get(id)?;
            matches!(node.tag()?, Tag::Input | Tag::Select)
                .then(|| node.attr(doc.interner(), "name").map(str::to_owned))
                .flatten()
        })
        .collect()
}

/// What kind of control an element is: "range", "checkbox", "text" or "select".
pub(super) fn input_kind(page: &Page, id: ElementId) -> Option<&'static str> {
    use lore_markup::InputType;
    let node = page.document().get(id)?;
    match (node.tag()?, node.form.input_type) {
        (Tag::Select, _) => Some("select"),
        (Tag::Input, InputType::Range) => Some("range"),
        (Tag::Input, InputType::Checkbox) => Some("checkbox"),
        (Tag::Input, InputType::Text) => Some("text"),
        _ => None,
    }
}

/// What a control holds, for a script.
enum Held {
    Number(f64),
    Boolean(bool),
    Text(String),
}

fn read_value(page: &Page, id: ElementId) -> Option<Held> {
    match input_kind(page, id)? {
        "range" => page.range_value(id).map(|n| Held::Number(f64::from(n))),
        "checkbox" => page.checked(id).map(Held::Boolean),
        _ => page.value(id).map(|text| Held::Text(text.to_owned())),
    }
}

fn held_value(lua: &Lua, held: Option<Held>) -> mlua::Result<Value> {
    Ok(match held {
        None => Value::Nil,
        Some(Held::Number(n)) => Value::Number(n),
        Some(Held::Boolean(b)) => Value::Boolean(b),
        Some(Held::Text(text)) => Value::String(lua.create_string(&text)?),
    })
}

/// Gives a control a value, or says why it cannot hold it.
fn write_value(page: &mut Page, id: ElementId, value: &Value, who: &str) -> Result<(), String> {
    let Some(kind) = input_kind(page, id) else {
        return Err(format!(
            "{who} holds no value, because it is not an input or a drop-down. To change what \
             it says, set its text: element.text = \"...\""
        ));
    };
    match (kind, value) {
        ("range", Value::Number(_) | Value::Integer(_)) => {
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            let number = match value {
                Value::Number(n) => *n as f32,
                Value::Integer(n) => *n as f32,
                _ => 0.0,
            };
            if !number.is_finite() {
                return Err(format!(
                    "{who} is a slider and was given a number with no size (infinity, or the \
                     answer to a sum such as 0 / 0). Give it a number like 50."
                ));
            }
            page.set_range_value(id, number);
            Ok(())
        }
        ("range", other) => Err(format!(
            "{who} is a slider, so its value is a number, but it was given {}. Write it like \
             this: .value = 50",
            describe(other)
        )),
        ("checkbox", Value::Boolean(on)) => {
            page.set_checked(id, *on);
            Ok(())
        }
        ("checkbox", other) => Err(format!(
            "{who} is a tick box, so its value is true or false, but it was given {}. Write \
             it like this: .value = true",
            describe(other)
        )),
        ("text", Value::String(text)) => {
            page.set_value(id, &text.to_string_lossy());
            Ok(())
        }
        ("select", Value::String(text)) => {
            let word = text.to_string_lossy();
            if super::choose_text(page, id, &word) {
                return Ok(());
            }
            let mut options = Vec::new();
            page.children(id, &mut options);
            let words: Vec<String> = options
                .iter()
                .map(|option| format!("\"{}\"", page.document().option_value(*option)))
                .collect();
            Err(format!(
                "{who} is a drop-down with no option \"{word}\". Its options are {}.",
                words.join(", ")
            ))
        }
        (_, other) => Err(format!(
            "{who} holds text, but it was given {}. Write it like this: .value = \"Ann\"",
            describe(other)
        )),
    }
}

fn is_inside(page: &Page, mut at: ElementId, outer: ElementId) -> bool {
    while let Some(parent) = page.document().parent(at) {
        if parent == outer {
            return true;
        }
        at = parent;
    }
    false
}

/// `camelCase` as a CSS name: `borderRadius` is `border-radius`. A custom
/// property (`--gold`) and a name with a dash in it are left alone.
fn css_name(key: &str) -> String {
    if key.starts_with("--") || key.contains('-') {
        return key.to_owned();
    }
    let mut out = String::with_capacity(key.len() + 2);
    for c in key.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A style value a script gave, as CSS text.
fn style_text(lua: &Lua, key: &str, value: &Value) -> mlua::Result<String> {
    match value {
        Value::Nil => Ok(String::new()),
        Value::String(text) => Ok(text.to_string_lossy()),
        Value::Integer(n) => Ok(n.to_string()),
        Value::Number(n) if n.is_finite() => Ok(n.to_string()),
        other => Err(fail(
            lua,
            format!(
                "style.{key} is text such as \"gold\" or \"12px\", but it was given {}. Write \
                 it like this: element.style.{key} = \"gold\". Setting it to nil takes it off.",
                describe(other)
            ),
        )),
    }
}

fn no_such_field(lua: &Lua, key: &str) -> mlua::Error {
    let mut message = format!("an element has no {key}.");
    let names = ELEMENT_FIELDS.iter().chain(ELEMENT_METHODS).copied();
    if let Some(near) = closest(key, names) {
        message.push_str(&format!(" Did you mean {near}?"));
    }
    message.push_str(&format!(
        " It has the fields {} and the functions {}.",
        ELEMENT_FIELDS.join(", "),
        ELEMENT_METHODS.join(", ")
    ));
    fail(lua, message)
}

fn elements_table(lua: &Lua, screen: &Rc<Shared>, found: &[ElementRef]) -> mlua::Result<Table> {
    let _ = screen;
    lua.create_sequence_from(
        found
            .iter()
            .map(|element| ElementValue(*element))
            .map(|element| lua.create_userdata(element))
            .collect::<mlua::Result<Vec<_>>>()?,
    )
}

/// `find` and `findAll` on a screen (`inside` is `None`) or an element.
pub(super) fn find_in(
    lua: &Lua,
    screen: ScreenId,
    inside: Option<ElementRef>,
    selector: &Value,
    all: bool,
) -> mlua::Result<Value> {
    let function = match (inside.is_some(), all) {
        (false, false) => "screen:find",
        (false, true) => "screen:findAll",
        (true, false) => "element:find",
        (true, true) => "element:findAll",
    };
    let selector = text_arg(
        lua,
        selector,
        function,
        &format!("{function}(\".menu button\")"),
    )?;
    let shared = shared_of(lua)?;
    let looked = shared.ui.with_page(screen, |page, generation| {
        let mut found = Vec::new();
        page.find_all(&selector, &mut found)?;
        if let Some(inside) = inside {
            found.retain(|&e| is_inside(page, e, inside.id));
        }
        found.retain(|&e| page.element_tag(e).is_some());
        Ok::<_, MarkupError>(
            found
                .into_iter()
                .map(|id| ElementRef {
                    screen,
                    id,
                    generation,
                })
                .collect::<Vec<_>>(),
        )
    });
    let found = match looked {
        Ok(Ok(found)) => found,
        Ok(Err(error)) => {
            return Err(markup_fail(
                lua,
                &format!("{function}(\"{selector}\") is not a selector this screen can use"),
                &error,
            ));
        }
        Err(why) => return Err(problem(lua, &why)),
    };
    if all {
        return Ok(Value::Table(elements_table(lua, &shared, &found)?));
    }
    match found.first() {
        Some(first) => Ok(Value::UserData(element(lua, *first)?)),
        None => Ok(Value::Nil),
    }
}

/// Runs `edit` on the element, turning every way it can go wrong into the
/// words a script's author is told.
fn edit<R>(
    lua: &Lua,
    this: &Value,
    method: &str,
    edit: impl FnOnce(&mut Page, ElementId) -> R,
) -> mlua::Result<R> {
    let element = this_of(lua, this, method)?;
    let shared = shared_of(lua)?;
    shared
        .ui
        .with_element(element, edit)
        .map_err(|why| problem(lua, &why))
}

fn class_name(lua: &Lua, value: &Value, method: &str) -> mlua::Result<String> {
    let name = text_arg(
        lua,
        value,
        method,
        &format!("element:{method}(\"selected\")"),
    )?;
    let name = name.trim().to_owned();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(fail(
            lua,
            format!(
                "element:{method} takes one class name, such as \"selected\", but it was \
                 given \"{name}\". Call it once for each class."
            ),
        ));
    }
    Ok(name)
}

fn add_class_methods<M: UserDataMethods<ElementValue>>(methods: &mut M) {
    methods.add_function("addClass", |lua, (this, name): (Value, Value)| {
        let name = class_name(lua, &name, "addClass")?;
        edit(lua, &this, "addClass", |page, id| page.add_class(id, &name)).map(|_| ())
    });
    methods.add_function("removeClass", |lua, (this, name): (Value, Value)| {
        let name = class_name(lua, &name, "removeClass")?;
        edit(lua, &this, "removeClass", |page, id| {
            page.remove_class(id, &name)
        })
        .map(|_| ())
    });
    methods.add_function("toggleClass", |lua, (this, name): (Value, Value)| {
        let name = class_name(lua, &name, "toggleClass")?;
        edit(lua, &this, "toggleClass", |page, id| {
            page.toggle_class(id, &name)
        })
        .map(|_| ())
    });
    methods.add_function("hasClass", |lua, (this, name): (Value, Value)| {
        let name = class_name(lua, &name, "hasClass")?;
        edit(lua, &this, "hasClass", |page, id| page.has_class(id, &name))
    });
}

impl UserData for ElementValue {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        add_class_methods(methods);
        methods.add_function(
            "setVar",
            |lua, (this, name, value): (Value, Value, Value)| {
                let name = text_arg(
                    lua,
                    &name,
                    "element:setVar",
                    "element:setVar(\"--gold\", \"#ffd75e\")",
                )?;
                let text = style_text(lua, &name, &value)?;
                edit(lua, &this, "setVar", |page, id| {
                    page.set_var(id, &name, &text)
                })?
                .map_err(|e| markup_fail(lua, &format!("element:setVar(\"{name}\") failed"), &e))
            },
        );
        methods.add_function("append", |lua, (this, html): (Value, Value)| {
            let html = text_arg(
                lua,
                &html,
                "element:append",
                "element:append(\"<p>Hello</p>\")",
            )?;
            edit(lua, &this, "append", |page, id| page.append_html(id, &html))?
                .map(|_| ())
                .map_err(|e| markup_fail(lua, "element:append could not add that HTML", &e))
        });
        methods.add_function("remove", |lua, this: Value| {
            edit(lua, &this, "remove", |page, id| page.remove(id)).map(|_| ())
        });
        methods.add_function("focus", |lua, this: Value| {
            let element = this_of(lua, &this, "focus")?;
            shared_of(lua)?
                .ui
                .ask_focus(element)
                .map_err(|why| problem(lua, &why))
        });
        methods.add_function("find", |lua, (this, selector): (Value, Value)| {
            let element = this_of(lua, &this, "find")?;
            find_in(lua, element.screen, Some(element), &selector, false)
        });
        methods.add_function("findAll", |lua, (this, selector): (Value, Value)| {
            let element = this_of(lua, &this, "findAll")?;
            find_in(lua, element.screen, Some(element), &selector, true)
        });
        methods.add_function("attribute", |lua, (this, name): (Value, Value)| {
            let name = text_arg(
                lua,
                &name,
                "element:attribute",
                "element:attribute(\"data-id\")",
            )?;
            let found = edit(lua, &this, "attribute", |page, id| {
                page.attribute(id, &name)
            })?;
            Ok(found)
        });
        methods.add_function(
            "setAttribute",
            |lua, (this, name, value): (Value, Value, Value)| {
                let name = text_arg(
                    lua,
                    &name,
                    "element:setAttribute",
                    "element:setAttribute(\"data-id\", \"7\")",
                )?;
                let text = match &value {
                    Value::String(text) => text.to_string_lossy(),
                    Value::Integer(n) => n.to_string(),
                    Value::Number(n) => n.to_string(),
                    Value::Boolean(on) => on.to_string(),
                    other => {
                        return Err(fail(
                            lua,
                            format!(
                                "element:setAttribute needs text, a number or true or false \
                                 for the value, but it was given {}. Write it like this: \
                                 element:setAttribute(\"data-id\", \"7\")",
                                describe(other)
                            ),
                        ));
                    }
                };
                edit(lua, &this, "setAttribute", |page, id| {
                    page.set_attribute(id, &name, &text)
                })?
                .map_err(|e| {
                    markup_fail(lua, &format!("element:setAttribute(\"{name}\") failed"), &e)
                })
            },
        );
        methods.add_function(
            "on",
            |lua, (this, kind, function): (Value, Value, Value)| on(lua, &this, &kind, &function),
        );
        methods.add_function("fill", |lua, (this, rows, make): (Value, Value, Value)| {
            fill(lua, &this, &rows, &make)
        });
        methods.add_meta_function(MetaMethod::ToString, |_, this: AnyUserData| {
            let element = this.borrow::<Self>()?.0;
            Ok(format!("element {}", element.id.index))
        });
        methods.add_meta_function(MetaMethod::Eq, |_, (a, b): (AnyUserData, AnyUserData)| {
            Ok(a.borrow::<Self>()?.0 == b.borrow::<Self>()?.0)
        });
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| {
                let element = this.borrow::<Self>()?.0;
                read_field(lua, element, &key_text(&key))
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (this, key, value): (AnyUserData, Value, Value)| {
                let element = this.borrow::<Self>()?.0;
                write_field(lua, element, &key_text(&key), &value)
            },
        );
    }
}

fn read_field(lua: &Lua, element: ElementRef, key: &str) -> mlua::Result<Value> {
    if !ELEMENT_FIELDS.contains(&key) {
        return Err(no_such_field(lua, key));
    }
    let shared = shared_of(lua)?;
    if key == "style" {
        shared
            .ui
            .with_element(element, |_, _| ())
            .map_err(|why| problem(lua, &why))?;
        return Ok(Value::UserData(lua.create_userdata(StyleProxy(element))?));
    }
    let generation = element.generation;
    let answer = shared
        .ui
        .with_element(element, |page, id| {
            let node = page.document().get(id);
            Ok::<_, mlua::Error>(match key {
                "text" => Answer::Text(page.text(id)),
                "value" => Answer::Held(read_value(page, id)),
                "checked" => Answer::Held(page.checked(id).map(Held::Boolean)),
                "hidden" => Answer::Flag(node.is_some_and(|n| n.hidden)),
                "disabled" => Answer::Flag(node.is_some_and(|n| n.disabled)),
                "class" => Answer::Text(page.attribute(id, "class").unwrap_or_default()),
                "id" => match node.and_then(|n| n.id) {
                    Some(atom) => Answer::Text(page.document().resolve(atom).to_owned()),
                    None => Answer::Nothing,
                },
                "tag" => Answer::Text(
                    page.element_tag(id)
                        .map_or(String::new(), |tag| tag.name().to_owned()),
                ),
                _ => page.parent(id).map_or(Answer::Nothing, Answer::Parent),
            })
        })
        .map_err(|why| problem(lua, &why))??;
    Ok(match answer {
        Answer::Text(text) => Value::String(lua.create_string(&text)?),
        Answer::Held(held) => held_value(lua, held)?,
        Answer::Flag(on) => Value::Boolean(on),
        Answer::Nothing => Value::Nil,
        Answer::Parent(id) => Value::UserData(self::element(
            lua,
            ElementRef {
                screen: element.screen,
                id,
                generation,
            },
        )?),
    })
}

enum Answer {
    Text(String),
    Held(Option<Held>),
    Flag(bool),
    Nothing,
    Parent(ElementId),
}

fn flag_of(lua: &Lua, key: &str, value: &Value) -> mlua::Result<bool> {
    match value {
        Value::Boolean(on) => Ok(*on),
        other => Err(fail(
            lua,
            format!(
                "element.{key} is true or false, but it was given {}. Write it like this: \
                 element.{key} = true",
                describe(other)
            ),
        )),
    }
}

fn write_field(lua: &Lua, element: ElementRef, key: &str, value: &Value) -> mlua::Result<()> {
    let shared = shared_of(lua)?;
    let wrong = |message: String| fail(lua, message);
    match key {
        "text" => {
            let text = match value {
                Value::String(text) => text.to_string_lossy(),
                Value::Integer(n) => n.to_string(),
                Value::Number(n) => n.to_string(),
                other => {
                    return Err(wrong(format!(
                        "element.text is text, but it was given {}. Write it like this: \
                         element.text = \"Start\"",
                        describe(other)
                    )));
                }
            };
            let set = shared
                .ui
                .with_element(element, |page, id| page.set_text(id, &text));
            match set {
                Ok(true) => Ok(()),
                Ok(false) => Err(wrong(
                    "this element holds no text (an input, picture or drop-down). To change an \
                     input, set element.value; to change a drop-down, set element.value to one \
                     of its options."
                        .to_owned(),
                )),
                Err(why) => Err(problem(lua, &why)),
            }
        }
        "value" => {
            let done = shared
                .ui
                .with_element(element, |page, id| {
                    write_value(page, id, value, "this element")
                })
                .map_err(|why| problem(lua, &why))?;
            done.map_err(wrong)
        }
        "checked" => {
            let on = flag_of(lua, key, value)?;
            let done = shared
                .ui
                .with_element(element, |page, id| page.set_checked(id, on))
                .map_err(|why| problem(lua, &why))?;
            if done {
                Ok(())
            } else {
                Err(wrong(
                    "only a tick box (<input type=\"checkbox\">) can be ticked. To change a \
                     slider or a text box, set element.value."
                        .to_owned(),
                ))
            }
        }
        "hidden" | "disabled" => {
            let on = flag_of(lua, key, value)?;
            let done = shared
                .ui
                .with_element(element, |page, id| {
                    page.set_attribute(id, key, if on { "true" } else { "false" })
                })
                .map_err(|why| problem(lua, &why))?;
            done.map_err(|e| markup_fail(lua, &format!("element.{key} could not be set"), &e))
        }
        "class" => {
            let text = text_arg(
                lua,
                value,
                "element.class",
                "element.class = \"big selected\"",
            )?;
            let done = shared
                .ui
                .with_element(element, |page, id| page.set_attribute(id, "class", &text))
                .map_err(|why| problem(lua, &why))?;
            done.map_err(|e| markup_fail(lua, "element.class could not be set", &e))
        }
        "id" | "tag" | "parent" => Err(wrong(format!(
            "element.{key} is read only: it says what the element is in the HTML, and cannot be \
             changed from the script. Find another element with screen:find(...), or change the \
             HTML file."
        ))),
        "style" => Err(wrong(
            "element.style is not set as a whole. Set one property at a time, like this: \
             element.style.color = \"gold\""
                .to_owned(),
        )),
        _ => Err(no_such_field(lua, key)),
    }
}

/// `element:on(kind, function)`.
fn on(lua: &Lua, this: &Value, kind: &Value, function: &Value) -> mlua::Result<()> {
    const EXAMPLE: &str = "element:on(\"click\", function(event) ... end)";
    let element = this_of(lua, this, "on")?;
    let kind = text_arg(lua, kind, "element:on", EXAMPLE)?;
    let (attribute, which) = match kind.as_str() {
        "click" => ("onclick", HandlerKind::Click),
        "change" => ("onchange", HandlerKind::Change),
        "input" => ("oninput", HandlerKind::Input),
        "focus" => ("onfocus", HandlerKind::Focus),
        other => {
            return Err(fail(
                lua,
                format!(
                    "element:on listens for \"click\", \"change\", \"input\" or \"focus\", but \
                     it was given \"{other}\". Write it like this: {EXAMPLE}"
                ),
            ));
        }
    };
    let Value::Function(function) = function else {
        return Err(fail(
            lua,
            format!(
                "element:on needs a function to call, but it was given {}. Write it like \
                 this: {EXAMPLE}",
                describe(function)
            ),
        ));
    };
    let shared = shared_of(lua)?;
    let written = shared
        .ui
        .with_element(element, |page, id| page.handler(id, which).is_some())
        .map_err(|why| problem(lua, &why))?;
    if !written {
        // No attribute names a handler yet: write one nobody else uses, so
        // the element fires. The function itself is kept for this element.
        let mut handlers = shared.ui_handlers.borrow_mut();
        let number = handlers.next_name;
        handlers.next_name += 1;
        drop(handlers);
        let name = format!("_ui_{number}_{kind}");
        let set = shared
            .ui
            .with_element(element, |page, id| page.set_attribute(id, attribute, &name))
            .map_err(|why| problem(lua, &why))?;
        set.map_err(|e| markup_fail(lua, "element:on could not listen there", &e))?;
    }
    shared.ui_handlers.borrow_mut().by_element.insert(
        (
            element.screen,
            element.id,
            element.generation,
            which.index(),
        ),
        function.clone(),
    );
    Ok(())
}

/// `element:fill(rows, function)`.
fn fill(lua: &Lua, this: &Value, rows: &Value, make: &Value) -> mlua::Result<()> {
    const EXAMPLE: &str = "list:fill(items, function(item, i) return \"<button>\" .. game.ui.escape(item) .. \"</button>\" end)";
    let element = this_of(lua, this, "fill")?;
    let Value::Table(rows) = rows else {
        return Err(fail(
            lua,
            format!(
                "element:fill needs a list of rows, but it was given {}. Write it like this: \
                 {EXAMPLE}",
                describe(rows)
            ),
        ));
    };
    let Value::Function(make) = make else {
        return Err(fail(
            lua,
            format!(
                "element:fill needs a function that gives the HTML of one row, but it was \
                 given {}. Write it like this: {EXAMPLE}",
                describe(make)
            ),
        ));
    };
    let mut html = String::new();
    for (index, row) in rows.sequence_values::<Value>().enumerate() {
        let row = row?;
        match make.call::<Value>((row, index + 1))? {
            Value::String(text) => html.push_str(&text.to_str()?),
            other => {
                return Err(fail(
                    lua,
                    format!(
                        "the function given to element:fill must return the row's HTML as \
                         text, but it returned {} for row {}. Write it like this: {EXAMPLE}",
                        describe(&other),
                        index + 1
                    ),
                ));
            }
        }
    }
    let shared = shared_of(lua)?;
    let done = shared
        .ui
        .with_element(element, |page, id| {
            page.set_text(id, "");
            let mut kids = Vec::new();
            page.children(id, &mut kids);
            for kid in kids {
                page.remove(kid);
            }
            page.append_html(id, &html)
        })
        .map_err(|why| problem(lua, &why))?;
    done.map(|_| ())
        .map_err(|e| markup_fail(lua, "element:fill could not add the rows' HTML", &e))
}

impl UserData for StyleProxy {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| {
                let element = this.borrow::<Self>()?.0;
                let property = css_name(&key_text(&key));
                let read = shared_of(lua)?
                    .ui
                    .with_element(element, |page, id| page.style_value(id, &property))
                    .map_err(|why| problem(lua, &why))?;
                match read {
                    Some(text) => Ok(Value::String(lua.create_string(&text)?)),
                    None => Ok(Value::Nil),
                }
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (this, key, value): (AnyUserData, Value, Value)| {
                let element = this.borrow::<Self>()?.0;
                let key = key_text(&key);
                let property = css_name(&key);
                let text = style_text(lua, &key, &value)?;
                let done = shared_of(lua)?
                    .ui
                    .with_element(element, |page, id| page.set_style(id, &property, &text))
                    .map_err(|why| problem(lua, &why))?;
                done.map_err(|e| {
                    markup_fail(
                        lua,
                        &format!("element.style.{key} = ... was not accepted"),
                        &e,
                    )
                })
            },
        );
    }
}

fn unknown_input(lua: &Lua, page: &Page, name: &str, file: &str) -> mlua::Error {
    let names = input_names(page);
    let mut message = format!("{file} has no input called \"{name}\".");
    if let Some(near) = closest(name, names.iter().map(String::as_str)) {
        message.push_str(&format!(" Did you mean \"{near}\"?"));
    }
    if names.is_empty() {
        message.push_str(
            " It has no named inputs. Name one in the HTML, like <input name=\"volume\">.",
        );
    } else {
        message.push_str(&format!(
            " Its inputs are: {}. Name an input in the HTML with name=\"...\".",
            names.join(", ")
        ));
    }
    fail(lua, message)
}

/// The input or drop-down of a page that has this `name`.
pub(super) fn find_input(page: &Page, name: &str) -> Option<ElementId> {
    let doc = page.document();
    doc.descendants(doc.root()).find(|&id| {
        doc.get(id).is_some_and(|node| {
            matches!(node.tag(), Some(Tag::Input | Tag::Select))
                && node.attr(doc.interner(), "name") == Some(name)
        })
    })
}

pub(super) fn unknown_input_error(lua: &Lua, screen: ScreenId, name: &str) -> mlua::Error {
    let Ok(shared) = shared_of(lua) else {
        return fail(lua, format!("there is no input called \"{name}\""));
    };
    let file = shared.ui.file(screen).unwrap_or_default();
    match shared
        .ui
        .with_page(screen, |page, _| unknown_input(lua, page, name, &file))
    {
        Ok(error) => error,
        Err(why) => problem(lua, &why),
    }
}

impl UserData for ValuesProxy {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(
            MetaMethod::Index,
            |lua, (this, key): (AnyUserData, Value)| {
                let screen = this.borrow::<Self>()?.0;
                let name = key_text(&key);
                let shared = shared_of(lua)?;
                let held = shared
                    .ui
                    .with_page(screen, |page, _| {
                        find_input(page, &name).map(|id| read_value(page, id))
                    })
                    .map_err(|why| problem(lua, &why))?;
                match held {
                    Some(held) => held_value(lua, held),
                    None => Err(unknown_input_error(lua, screen, &name)),
                }
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |lua, (this, key, value): (AnyUserData, Value, Value)| {
                let screen = this.borrow::<Self>()?.0;
                let name = key_text(&key);
                let shared = shared_of(lua)?;
                let done = shared
                    .ui
                    .with_page(screen, |page, _| {
                        find_input(page, &name).map(|id| {
                            write_value(page, id, &value, &format!("the input \"{name}\""))
                        })
                    })
                    .map_err(|why| problem(lua, &why))?;
                match done {
                    Some(Ok(())) => Ok(()),
                    Some(Err(message)) => Err(fail(lua, message)),
                    None => Err(unknown_input_error(lua, screen, &name)),
                }
            },
        );
    }
}
