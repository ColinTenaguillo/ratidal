//! Remapping the command keys.
//!
//! The keys are not a table: each is guarded by what is on screen -- `t` is
//! the next search tab or the next home tab, `l` moves along a row or into
//! an artist's sections -- and a table of key-to-action would have to carry
//! all of that or lose it.
//!
//! So this remaps rather than dispatches. A pressed key is translated to
//! the default key it stands for, and the match downstream is untouched.
//! Everything keeps working, including the guards, and a rebind is one line
//! of config.
//!
//! Text input is deliberately not remappable: while the search box has the
//! keyboard, `q` is a letter rather than a command, and a config that could
//! change that would be a config that breaks typing.

use std::collections::HashMap;

use crossterm::event::KeyCode;

/// What the user asked for, as `pressed -> default`.
///
/// Empty is the normal case, and costs a hash lookup of nothing.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    to_default: HashMap<KeyCode, KeyCode>,
    /// Defaults whose action moved to another key, and which therefore do
    /// nothing now. Kept apart from the map above so a default rebound onto
    /// itself is not mistaken for one that was vacated.
    rebound_away: std::collections::HashSet<KeyCode>,
}

impl Keymap {
    /// Build from the config's `action = "key"` pairs.
    ///
    /// Named by action rather than by default key, since "what does `o` do"
    /// is not what someone editing a config is asking -- they know the
    /// action they want and are choosing a key for it.
    ///
    /// Unknown actions and unparseable keys are reported rather than
    /// ignored: a typo in a config that silently does nothing is worse than
    /// one that says so.
    pub fn from_config(binds: &HashMap<String, String>) -> (Self, Vec<String>) {
        let mut to_default = HashMap::new();
        let mut rebound_away = std::collections::HashSet::new();
        let mut problems = Vec::new();

        for (action, key) in binds {
            let Some(default) = default_key(action) else {
                problems.push(format!("unknown action {action:?}"));
                continue;
            };
            let Some(pressed) = parse_key(key) else {
                problems.push(format!("cannot read key {key:?} for {action:?}"));
                continue;
            };
            if let Some(taken) = to_default.insert(pressed, default) {
                problems.push(format!(
                    "{key:?} is bound twice, to {action:?} and to {:?}",
                    action_of(taken).unwrap_or("something else")
                ));
            }
            // The default this action left behind, unless it stayed put.
            if pressed != default {
                rebound_away.insert(default);
            }
        }
        // A default that something else was bound to is not vacant: it is
        // now that other action's key.
        rebound_away.retain(|k| !to_default.contains_key(k));
        (Self { to_default, rebound_away }, problems)
    }

    /// The key the app should act on.
    ///
    /// A default that has been rebound elsewhere stops working: binding
    /// `quit` to `x` moves it there rather than adding a second key for it.
    /// Someone who rebinds `q` away is usually doing it to get `q` back for
    /// something else, and a default that kept firing would be in the way.
    pub fn resolve(&self, pressed: KeyCode) -> KeyCode {
        if let Some(default) = self.to_default.get(&pressed) {
            return *default;
        }
        if self.rebound_away.contains(&pressed) {
            // Nothing: a key whose action moved is now unbound.
            return KeyCode::Null;
        }
        pressed
    }

    /// Whether anything was rebound, for the help view to say so.
    pub fn is_empty(&self) -> bool {
        self.to_default.is_empty()
    }
}

/// Every action a key can be bound to, and the key it has by default.
///
/// One place, so the config, the help and the key handling cannot disagree
/// about what exists.
pub const ACTIONS: &[(&str, char)] = &[
    ("down", 'j'),
    ("up", 'k'),
    ("left", 'h'),
    ("right", 'l'),
    ("sidebar_next", 'J'),
    ("sidebar_previous", 'K'),
    ("back", '['),
    ("forward", ']'),
    ("filter", '/'),
    ("search", 's'),
    ("see_all", 'o'),
    ("biography", 'b'),
    ("track_radio", 'R'),
    ("artist_radio", 'S'),
    ("next_tab", 't'),
    ("play_pause", ' '),
    ("favourite", 'A'),
    ("queue_next", 'n'),
    ("queue_previous", 'p'),
    ("shuffle", 'z'),
    ("repeat", 'r'),
    ("help", '?'),
    ("quit", 'q'),
];

fn default_key(action: &str) -> Option<KeyCode> {
    ACTIONS
        .iter()
        .find(|(name, _)| *name == action)
        .map(|(_, c)| KeyCode::Char(*c))
}

fn action_of(key: KeyCode) -> Option<&'static str> {
    let KeyCode::Char(c) = key else { return None };
    ACTIONS
        .iter()
        .find(|(_, default)| *default == c)
        .map(|(name, _)| *name)
}

/// Read a key from the config.
///
/// A single character is itself; the few names are for the keys that have
/// no character to write.
fn parse_key(s: &str) -> Option<KeyCode> {
    match s {
        "space" => return Some(KeyCode::Char(' ')),
        "tab" => return Some(KeyCode::Tab),
        "enter" => return Some(KeyCode::Enter),
        "esc" => return Some(KeyCode::Esc),
        _ => {}
    }
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(KeyCode::Char(c)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> (Keymap, Vec<String>) {
        let binds: HashMap<String, String> = pairs
            .iter()
            .map(|(a, k)| ((*a).to_string(), (*k).to_string()))
            .collect();
        Keymap::from_config(&binds)
    }

    #[test]
    fn an_empty_config_changes_nothing() {
        let (keymap, problems) = map(&[]);
        assert!(problems.is_empty());
        assert!(keymap.is_empty());
        // Every key resolves to itself.
        for (_, default) in ACTIONS {
            let key = KeyCode::Char(*default);
            assert_eq!(keymap.resolve(key), key);
        }
    }

    #[test]
    fn a_rebound_key_resolves_to_the_default_it_stands_for() {
        // The app's own match is untouched: what changes is which key
        // arrives at it.
        let (keymap, problems) = map(&[("quit", "x")]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(keymap.resolve(KeyCode::Char('x')), KeyCode::Char('q'));
        // And a key nobody rebound is still itself.
        assert_eq!(keymap.resolve(KeyCode::Char('j')), KeyCode::Char('j'));
    }

    #[test]
    fn a_default_whose_action_moved_stops_working() {
        // Binding `quit` to `x` moves it rather than adding a second key:
        // someone who rebinds `q` away is usually doing it to get `q` back,
        // and a default that kept firing would be in the way.
        let (keymap, _) = map(&[("quit", "x")]);
        assert_eq!(
            keymap.resolve(KeyCode::Char('q')),
            KeyCode::Null,
            "the vacated default does nothing"
        );

        // Unless something else took it: swapping two actions leaves both
        // keys working, each doing the other's job.
        let (keymap, problems) = map(&[("quit", "s"), ("search", "q")]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(keymap.resolve(KeyCode::Char('s')), KeyCode::Char('q'));
        assert_eq!(keymap.resolve(KeyCode::Char('q')), KeyCode::Char('s'));

        // And binding an action to the key it already has changes nothing.
        let (keymap, _) = map(&[("quit", "q")]);
        assert_eq!(keymap.resolve(KeyCode::Char('q')), KeyCode::Char('q'));
    }

    #[test]
    fn the_named_keys_are_the_ones_with_no_character_to_write() {
        let (keymap, problems) = map(&[("play_pause", "tab")]);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(keymap.resolve(KeyCode::Tab), KeyCode::Char(' '));

        let (keymap, _) = map(&[("quit", "space")]);
        assert_eq!(keymap.resolve(KeyCode::Char(' ')), KeyCode::Char('q'));
    }

    #[test]
    fn a_typo_is_reported_rather_than_ignored() {
        // A config that silently does nothing is worse than one that says
        // what it could not read.
        let (_, problems) = map(&[("qiut", "x")]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("qiut"), "{problems:?}");

        let (_, problems) = map(&[("quit", "ctrl-x")]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("ctrl-x"), "{problems:?}");
    }

    #[test]
    fn one_key_bound_to_two_actions_is_reported() {
        // Whichever won, the other would silently stop working.
        let (_, problems) = map(&[("quit", "x"), ("search", "x")]);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("bound twice"), "{problems:?}");
    }

    #[test]
    fn every_action_name_is_unique_and_its_default_is_too() {
        // Two actions on one default would make `action_of` ambiguous, and
        // two names would make the config's meaning depend on order.
        let mut names: Vec<&str> = ACTIONS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "an action name is repeated");

        let mut keys: Vec<char> = ACTIONS.iter().map(|(_, k)| *k).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "a default key is repeated");
    }
}
