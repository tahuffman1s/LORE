//! The bindings file: a player's own controls as plain text (TOML).
//!
//! The file holds only what the player changed. An action it does not mention
//! keeps the game's own bindings, so a game can add an action or change a
//! default later without the file going stale.
//!
//! ```toml
//! [bindings]
//! jump = ["F", "GamepadSouth"]
//! # move = ["WASD", "LeftStick"]
//! ```

use std::fmt;
use std::fmt::Write as _;

use toml::de::{DeTable, DeValue};

use crate::binding::{ActionKind, Binding};
use crate::map::ActionMap;
use crate::names::UnknownName;

/// The table of the file that holds the bindings.
const TABLE: &str = "bindings";

/// What a written file says before its bindings.
const HEADER: &str = "\
# Controls. Change a line and save this file: a game that is running
# uses it at once, and every other game when it starts.
#
# A line that starts with # is not read: that action keeps the game's own
# controls. To change one, take away the # and edit the names in its list.
# Keep one name for keyboard or mouse and one for gamepad in every list.
#
# Names are a key (\"Space\", \"F\", \"LeftShift\"), a mouse button (\"MouseLeft\",
# \"MouseWheelUp\") or a gamepad button (\"GamepadSouth\", \"RightTrigger\").
# Moving and looking take \"WASD\", \"ArrowKeys\", \"LeftStick\", \"RightStick\" or
# \"MouseMotion\"; four keys of your own are \"I/K/J/L\": up, down, left, right.
";

/// Why a bindings file could not be read at all: it is not valid TOML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingsError {
    /// The line of the mistake, counted from 1.
    pub line: usize,
    /// The column of the mistake, counted from 1.
    pub column: usize,
    /// What the TOML reader expected there.
    pub message: String,
}

impl fmt::Display for BindingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Line {}, column {}: {}. A binding is written like jump = [\"Space\", \
             \"GamepadSouth\"].",
            self.line,
            self.column,
            self.message.trim_end_matches('.')
        )
    }
}

impl std::error::Error for BindingsError {}

/// Actions the standard controls once had. A bindings file written then
/// names them, and is told they are gone rather than asked whether it
/// meant some other action. A game that makes an action of its own by one
/// of these names is not affected.
const TAKEN_OUT: [&str; 1] = ["scoreboard"];

/// Actions of the standard controls that have a new name, old name first.
/// A bindings file written before still works: what it binds under the old
/// name goes to the new one, and the player is told what to call the line.
/// A game that makes an action of its own by an old name is not affected.
const RENAMED: [(&str, &str); 3] = [
    ("grenade", "use_item"),
    ("next_weapon", "next_item"),
    ("previous_weapon", "previous_item"),
];

/// One entry of a bindings file that was left out, or that the player
/// should change, and why.
///
/// The rest of the file is still used. Each one prints as a sentence saying
/// what to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingsProblem {
    /// The line of the entry, counted from 1.
    pub line: usize,
    /// What is wrong with it.
    pub kind: ProblemKind,
}

/// What is wrong with one entry of a bindings file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProblemKind {
    /// Something other than the `[bindings]` table.
    NotBindings {
        /// The name that was found.
        name: String,
    },
    /// The game has no action of this name.
    UnknownAction(UnknownName),
    /// The standard controls had an action of this name once and no longer
    /// do, so a file written back then still lists it.
    TakenOut {
        /// The action that is gone.
        action: String,
    },
    /// The standard controls call this action something else now. What the
    /// line binds is used, for the action under its new name.
    Renamed {
        /// The name the file uses.
        old: String,
        /// The name the action has now.
        new: String,
    },
    /// The file lists an action under the name it used to have and under
    /// the name it has now. The line with the old name is left out.
    RenamedTwice {
        /// The name the action used to have.
        old: String,
        /// The name the action has now.
        new: String,
    },
    /// The entry is not a list of names.
    NotAList {
        /// The action being bound.
        action: String,
    },
    /// A name in the list is not a key, button or stick.
    UnknownBinding {
        /// The action being bound.
        action: String,
        /// Which name failed, with a suggestion when one is close.
        source: UnknownName,
    },
    /// A name in the list cannot drive this kind of action.
    WrongKind {
        /// The action being bound.
        action: String,
        /// The action's kind.
        kind: ActionKind,
        /// The binding that does not fit.
        binding: Binding,
    },
}

impl fmt::Display for BindingsProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Line {}: ", self.line)?;
        match &self.kind {
            ProblemKind::NotBindings { name } => write!(
                f,
                "\"{name}\" is not read. Bindings go under a line saying [{TABLE}], \
                 like jump = [\"Space\", \"GamepadSouth\"]."
            ),
            ProblemKind::UnknownAction(unknown) => {
                write!(f, "{unknown} The line is not used.")
            }
            ProblemKind::TakenOut { action } => write!(
                f,
                "\"{action}\" is no longer an action: LORE keeps no scores, so there is no \
                 scoreboard to show. Delete this line. The rest of the file is used."
            ),
            ProblemKind::Renamed { old, new } => write!(
                f,
                "\"{old}\" is now called \"{new}\". What this line binds still works; \
                 change the name at the start of the line to {new} and this note goes away."
            ),
            ProblemKind::RenamedTwice { old, new } => write!(
                f,
                "\"{old}\" is now called \"{new}\", and this file has a line for {new} \
                 too, which is the one used. Delete this line."
            ),
            ProblemKind::NotAList { action } => write!(
                f,
                "\"{action}\" needs a list of names in square brackets, like \
                 {action} = [\"Space\", \"GamepadSouth\"]. It keeps the game's own controls."
            ),
            ProblemKind::UnknownBinding { action, source } => write!(
                f,
                "{source} Action \"{action}\" keeps the game's own controls."
            ),
            ProblemKind::WrongKind {
                action,
                kind,
                binding,
            } => write!(
                f,
                "Action \"{action}\" is {}, but \"{binding}\" is {}. Pick a different name; \
                 until then \"{action}\" keeps the game's own controls.",
                kind.describe(),
                binding.kind().describe(),
            ),
        }
    }
}

impl ActionMap {
    /// Applies the text of a bindings file: each action it lists gets the
    /// bindings written there, and every other action keeps what it has.
    ///
    /// A player's file should never stop the game from starting, so an entry
    /// that cannot be used is left out and reported; the action it names
    /// keeps its bindings. Show the returned problems to the player.
    ///
    /// ```
    /// use lore_input::ActionMap;
    ///
    /// let mut map = ActionMap::standard();
    /// let problems = map.read_bindings("[bindings]\njump = [\"F\", \"GamepadSouth\"]")?;
    /// assert!(problems.is_empty());
    /// # Ok::<(), lore_input::BindingsError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Fails, changing nothing, if the text is not valid TOML.
    pub fn read_bindings(&mut self, text: &str) -> Result<Vec<BindingsProblem>, BindingsError> {
        let document = DeTable::parse(text).map_err(|error| {
            let (line, column) = position(text, error.span().map_or(0, |span| span.start));
            BindingsError {
                line,
                column,
                message: error.message().to_owned(),
            }
        })?;

        let mut problems = Vec::new();
        for (key, value) in document.get_ref() {
            let line = position(text, key.span().start).0;
            match value.get_ref() {
                DeValue::Table(entries) if key.get_ref() == TABLE => {
                    for (action, list) in entries {
                        let line = position(text, action.span().start).0;
                        let name: &str = action.get_ref();
                        let result = match self.renamed(name) {
                            Some(new) if entries.keys().any(|key| key.get_ref() == new) => {
                                Err(ProblemKind::RenamedTwice {
                                    old: name.to_owned(),
                                    new: new.to_owned(),
                                })
                            }
                            // The line is read as if it had the new name,
                            // and anything wrong in it is said of that name.
                            Some(new) => self.read_entry(new, list.get_ref()).and(Err(
                                ProblemKind::Renamed {
                                    old: name.to_owned(),
                                    new: new.to_owned(),
                                },
                            )),
                            None => self.read_entry(name, list.get_ref()),
                        };
                        if let Err(kind) = result {
                            problems.push(BindingsProblem { line, kind });
                        }
                    }
                }
                _ => problems.push(BindingsProblem {
                    line,
                    kind: ProblemKind::NotBindings {
                        name: key.get_ref().to_string(),
                    },
                }),
            }
        }
        // The reader hands entries over by name; a player reads top to bottom.
        problems.sort_by_key(|problem| problem.line);
        Ok(problems)
    }

    /// The name the standard controls now have for `name`, if `name` is what
    /// one of them used to be called and this map has no action by it.
    fn renamed(&self, name: &str) -> Option<&'static str> {
        let &(_, new) = RENAMED.iter().find(|(old, _)| *old == name)?;
        (self.action(name).is_err() && self.action(new).is_ok()).then_some(new)
    }

    /// Gives the action called `name` the bindings in `list`, or says why not.
    fn read_entry(&mut self, name: &str, list: &DeValue<'_>) -> Result<(), ProblemKind> {
        let action = self.action(name).map_err(|unknown| {
            if TAKEN_OUT.contains(&name) {
                ProblemKind::TakenOut {
                    action: name.to_owned(),
                }
            } else {
                ProblemKind::UnknownAction(unknown)
            }
        })?;
        let not_a_list = || ProblemKind::NotAList {
            action: name.to_owned(),
        };
        let kind = self.kind(action).ok_or_else(not_a_list)?;
        let list = list.as_array().ok_or_else(not_a_list)?;

        let mut bindings = Vec::with_capacity(list.len());
        for item in list.iter() {
            let text = item.get_ref().as_str().ok_or_else(not_a_list)?;
            let binding: Binding = text.parse().map_err(|source| ProblemKind::UnknownBinding {
                action: name.to_owned(),
                source,
            })?;
            if binding.kind() != kind {
                return Err(ProblemKind::WrongKind {
                    action: name.to_owned(),
                    kind,
                    binding,
                });
            }
            bindings.push(binding);
        }
        // Every binding was checked against the action's kind just above.
        let _ = self.set_bindings(action, bindings);
        Ok(())
    }

    /// The text of a bindings file for this map.
    ///
    /// Every action is listed, so the file shows a player what there is to
    /// change, but an action bound as it is in `defaults` is written as a
    /// comment: only what differs is read back by
    /// [`read_bindings`](Self::read_bindings).
    ///
    /// ```
    /// use lore_input::ActionMap;
    ///
    /// let defaults = ActionMap::standard();
    /// let mut map = defaults.clone();
    /// map.read_bindings("[bindings]\njump = [\"F\", \"GamepadSouth\"]")?;
    ///
    /// let text = map.write_bindings(&defaults);
    /// assert!(text.contains("\njump = [\"F\", \"GamepadSouth\"]\n"));
    /// assert!(text.contains("\n# fire = [\"MouseLeft\", \"RightTrigger\"]\n"));
    /// # Ok::<(), lore_input::BindingsError>(())
    /// ```
    #[must_use]
    pub fn write_bindings(&self, defaults: &Self) -> String {
        let mut text = format!("{HEADER}\n[{TABLE}]\n");
        for action in self.actions() {
            let name = self.name(action);
            let bindings = self.bindings(action);
            let unchanged = defaults
                .action(name)
                .is_ok_and(|default| defaults.bindings(default) == bindings);
            if unchanged {
                text.push_str("# ");
            }
            write_key(&mut text, name);
            text.push_str(" = [");
            for (index, binding) in bindings.iter().enumerate() {
                let comma = if index == 0 { "" } else { ", " };
                // Writing to a `String` cannot fail.
                let _ = write!(text, "{comma}\"{binding}\"");
            }
            text.push_str("]\n");
        }
        text
    }
}

/// Writes an action's name as a TOML key, in quotes only when it needs them.
fn write_key(text: &mut String, name: &str) {
    let bare = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if bare {
        text.push_str(name);
        return;
    }
    text.push('"');
    for c in name.chars() {
        match c {
            '"' => text.push_str("\\\""),
            '\\' => text.push_str("\\\\"),
            // Writing to a `String` cannot fail.
            c if c.is_control() => {
                let _ = write!(text, "\\u{:04X}", u32::from(c));
            }
            c => text.push(c),
        }
    }
    text.push('"');
}

/// The line and column, both counted from 1, of the byte at `offset`.
fn position(text: &str, offset: usize) -> (usize, usize) {
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit('\n')
        .next()
        .map_or(0, |last| last.chars().count())
        + 1;
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devices::{Key, Stick};

    fn said(problems: &[BindingsProblem]) -> Vec<String> {
        problems.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_file_changes_only_the_actions_it_lists() {
        let defaults = ActionMap::standard();
        let mut map = defaults.clone();
        let problems = map
            .read_bindings(
                "# my controls\n[bindings]\njump = ['f', \"GamepadSouth\"]\n\
                 move = [\n  \"I/K/J/L\",\n  \"LeftStick\",\n]\nmelee = []\n",
            )
            .unwrap();
        assert_eq!(problems, Vec::new());

        let jump = map.action("jump").unwrap();
        assert_eq!(map.bindings(jump)[0], Binding::Button(Key::F.into()));
        let movement = map.action("move").unwrap();
        assert_eq!(
            map.bindings(movement),
            [
                Binding::directions(Key::I, Key::K, Key::J, Key::L),
                Binding::Stick(Stick::LeftStick)
            ]
        );
        assert!(map.bindings(map.action("melee").unwrap()).is_empty());
        let fire = map.action("fire").unwrap();
        assert_eq!(map.bindings(fire), defaults.bindings(fire));
    }

    #[test]
    fn an_empty_file_changes_nothing() {
        let mut map = ActionMap::standard();
        assert_eq!(map.read_bindings(""), Ok(Vec::new()));
        assert_eq!(map.read_bindings("[bindings]\n"), Ok(Vec::new()));
        assert_eq!(map, ActionMap::standard());
    }

    #[test]
    fn a_written_file_reads_back_to_the_same_map() {
        let defaults = ActionMap::standard();
        let mut map = defaults.clone();
        map.replace_button(
            map.action("move").unwrap(),
            Key::W.into(),
            Key::ArrowUp.into(),
        );
        map.set_bindings(map.action("use_item").unwrap(), [])
            .unwrap();

        let text = map.write_bindings(&defaults);
        assert!(
            text.contains("\nmove = [\"ArrowUp/S/A/D\", \"LeftStick\"]\n"),
            "{text}"
        );
        assert!(text.contains("\nuse_item = []\n"), "{text}");
        assert!(
            text.contains("\n# jump = [\"Space\", \"GamepadSouth\"]\n"),
            "{text}"
        );

        let mut read = defaults.clone();
        assert_eq!(read.read_bindings(&text), Ok(Vec::new()));
        assert_eq!(read, map);

        // With nothing changed, nothing is read back.
        let untouched = defaults.write_bindings(&defaults);
        let lines = untouched.lines().filter(|line| !line.starts_with('#'));
        assert_eq!(lines.collect::<Vec<_>>(), ["", "[bindings]"]);
    }

    #[test]
    fn the_names_the_header_teaches_are_real() {
        for name in HEADER.split('"').skip(1).step_by(2) {
            assert!(name.parse::<Binding>().is_ok(), "{name}");
        }
    }

    #[test]
    fn odd_action_names_are_written_as_valid_keys() {
        let mut map = ActionMap::new();
        map.button("say \"hi\"", ["Enter"]).unwrap();
        map.button("jump", ["Space"]).unwrap();
        let text = map.write_bindings(&ActionMap::new());
        assert!(
            text.contains("\n\"say \\\"hi\\\"\" = [\"Enter\"]\n"),
            "{text}"
        );

        let mut read = ActionMap::new();
        read.button("say \"hi\"", ["Tab"]).unwrap();
        read.button("jump", ["Tab"]).unwrap();
        assert_eq!(read.read_bindings(&text), Ok(Vec::new()));
        assert_eq!(read, map);
    }

    #[test]
    fn entries_that_cannot_be_used_are_left_out_and_explained() {
        let defaults = ActionMap::standard();
        let mut map = defaults.clone();
        let problems = map
            .read_bindings(
                "volume = 3\n\
                 [bindings]\n\
                 jmup = [\"F\"]\n\
                 jump = [\"Spcae\", \"GamepadSouth\"]\n\
                 fire = \"MouseLeft\"\n\
                 aim = [\"LeftStick\"]\n\
                 reload = [\"T\", \"GamepadWest\"]\n\
                 crouch = [1]\n",
            )
            .unwrap();
        assert_eq!(
            said(&problems),
            [
                "Line 1: \"volume\" is not read. Bindings go under a line saying [bindings], \
                 like jump = [\"Space\", \"GamepadSouth\"].",
                "Line 3: \"jmup\" is not a known action. Did you mean \"jump\"? The line is \
                 not used.",
                "Line 4: \"Spcae\" is not a known binding. Did you mean \"Space\"? Action \
                 \"jump\" keeps the game's own controls.",
                "Line 5: \"fire\" needs a list of names in square brackets, like fire = \
                 [\"Space\", \"GamepadSouth\"]. It keeps the game's own controls.",
                "Line 6: Action \"aim\" is a button, but \"LeftStick\" is a 2D axis (a \
                 direction). Pick a different name; until then \"aim\" keeps the game's own \
                 controls.",
                "Line 8: \"crouch\" needs a list of names in square brackets, like crouch = \
                 [\"Space\", \"GamepadSouth\"]. It keeps the game's own controls.",
            ]
        );

        // The one good line was used; every bad one left its action alone.
        let reload = map.action("reload").unwrap();
        assert_eq!(map.bindings(reload)[0], Binding::Button(Key::T.into()));
        for name in ["jump", "fire", "aim", "crouch"] {
            let action = map.action(name).unwrap();
            assert_eq!(map.bindings(action), defaults.bindings(action), "{name}");
        }
    }

    #[test]
    fn a_file_from_when_there_was_a_scoreboard_still_loads_and_says_to_delete_the_line() {
        let mut map = ActionMap::standard();
        let problems = map
            .read_bindings(
                "[bindings]\n\
                 jump = [\"J\", \"GamepadSouth\"]\n\
                 scoreboard = [\"Tab\", \"GamepadSelect\"]\n\
                 reload = [\"T\", \"GamepadWest\"]\n",
            )
            .unwrap();
        assert_eq!(
            said(&problems),
            [
                "Line 3: \"scoreboard\" is no longer an action: LORE keeps no scores, so there \
              is no scoreboard to show. Delete this line. The rest of the file is used."
            ]
        );
        // The lines around it took effect.
        let bound = |map: &ActionMap, name: &str| map.bindings(map.action(name).unwrap()).to_vec();
        assert_eq!(bound(&map, "jump")[0].to_string(), "J");
        assert_eq!(bound(&map, "reload")[0].to_string(), "T");

        // A game with a scoreboard action of its own binds it as any other.
        let mut own = ActionMap::standard();
        own.button("scoreboard", ["Tab", "GamepadSelect"]).unwrap();
        let problems = own
            .read_bindings("[bindings]\nscoreboard = [\"M\", \"GamepadSelect\"]\n")
            .unwrap();
        assert_eq!(said(&problems), [] as [&str; 0]);
        assert_eq!(bound(&own, "scoreboard")[0].to_string(), "M");
    }

    #[test]
    fn a_file_with_the_old_names_of_actions_still_binds_them_and_says_the_new_names() {
        let mut map = ActionMap::standard();
        let problems = map
            .read_bindings(
                "[bindings]\n\
                 grenade = [\"H\", \"LeftBumper\"]\n\
                 jump = [\"F\", \"GamepadSouth\"]\n\
                 next_weapon = [\"X\", \"DpadRight\"]\n\
                 previous_weapon = [\"Z\", \"DpadLeft\"]\n",
            )
            .unwrap();
        assert_eq!(
            said(&problems),
            [
                "Line 2: \"grenade\" is now called \"use_item\". What this line binds still \
                 works; change the name at the start of the line to use_item and this note \
                 goes away.",
                "Line 4: \"next_weapon\" is now called \"next_item\". What this line binds \
                 still works; change the name at the start of the line to next_item and this \
                 note goes away.",
                "Line 5: \"previous_weapon\" is now called \"previous_item\". What this line \
                 binds still works; change the name at the start of the line to \
                 previous_item and this note goes away.",
            ]
        );
        let bound = |map: &ActionMap, name: &str| -> Vec<String> {
            (map.bindings(map.action(name).unwrap()).iter())
                .map(ToString::to_string)
                .collect()
        };
        assert_eq!(bound(&map, "use_item"), ["H", "LeftBumper"]);
        assert_eq!(bound(&map, "next_item"), ["X", "DpadRight"]);
        assert_eq!(bound(&map, "previous_item"), ["Z", "DpadLeft"]);
        assert_eq!(bound(&map, "jump"), ["F", "GamepadSouth"]);
        // The old names did not become actions.
        assert!(map.action("grenade").is_err());

        // Written out again, the file has the new names only, and reads
        // back to the same controls with nothing to say.
        let text = map.write_bindings(&ActionMap::standard());
        assert!(
            text.contains("\nuse_item = [\"H\", \"LeftBumper\"]\n"),
            "{text}"
        );
        assert!(!text.contains("grenade"), "{text}");
        let mut again = ActionMap::standard();
        assert_eq!(said(&again.read_bindings(&text).unwrap()), [] as [&str; 0]);
        assert_eq!(again, map);

        // A mistake in a line with an old name is said of the new name, and
        // nothing is bound.
        let mut map = ActionMap::standard();
        let problems = map.read_bindings("[bindings]\ngrenade = \"H\"\n").unwrap();
        assert_eq!(
            said(&problems),
            [
                "Line 2: \"use_item\" needs a list of names in square brackets, like \
                 use_item = [\"Space\", \"GamepadSouth\"]. It keeps the game's own controls."
            ]
        );
        assert_eq!(map, ActionMap::standard());
    }

    #[test]
    fn a_file_with_both_the_old_and_the_new_name_uses_the_new_one() {
        // Whichever order the lines are in.
        for text in [
            "[bindings]\ngrenade = [\"H\"]\nuse_item = [\"K\", \"RightBumper\"]\n",
            "[bindings]\nuse_item = [\"K\", \"RightBumper\"]\ngrenade = [\"H\"]\n",
        ] {
            let mut map = ActionMap::standard();
            let problems = map.read_bindings(text).unwrap();
            let line = if text.find("grenade") < text.find("use_item") {
                2
            } else {
                3
            };
            assert_eq!(
                said(&problems),
                [format!(
                    "Line {line}: \"grenade\" is now called \"use_item\", and this file has \
                     a line for use_item too, which is the one used. Delete this line."
                )]
            );
            let bound = map.bindings(map.action("use_item").unwrap());
            assert_eq!(bound[0].to_string(), "K");
        }
    }

    #[test]
    fn a_game_with_an_action_of_its_own_by_an_old_name_keeps_it() {
        let mut own = ActionMap::standard();
        own.button("grenade", ["H", "LeftBumper"]).unwrap();
        let problems = own
            .read_bindings("[bindings]\ngrenade = [\"B\", \"LeftBumper\"]\n")
            .unwrap();
        assert_eq!(said(&problems), [] as [&str; 0]);
        assert_eq!(
            own.bindings(own.action("grenade").unwrap())[0].to_string(),
            "B"
        );
        let standard = ActionMap::standard();
        let use_item = standard.action("use_item").unwrap();
        assert_eq!(own.bindings(use_item), standard.bindings(use_item));
    }

    #[test]
    fn text_that_is_not_toml_says_where_and_changes_nothing() {
        let mut map = ActionMap::standard();
        let error = map
            .read_bindings("[bindings]\njump = [\"F\"\nfire = [\"G\"]\n")
            .unwrap_err();
        assert_eq!(error.line, 3);
        let said = error.to_string();
        assert!(said.starts_with("Line 3, column "), "{said}");
        assert!(
            said.ends_with("A binding is written like jump = [\"Space\", \"GamepadSouth\"]."),
            "{said}"
        );
        assert_eq!(map, ActionMap::standard());
    }
}
