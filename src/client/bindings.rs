//! Remappable key bindings.
//!
//! Every input the game reads is an [`Action`]. Each action has up to two
//! [`Binding`]s: a key or mouse button, plus any modifiers that must be held
//! with it ("Ctrl+LMB"). Bindings are written as text, both in
//! `keybinds.ron` and in on-screen prompts.
//!
//! When several bindings share a button, the most specific chord whose
//! modifiers are all held wins. With "LMB" on grapple and "Ctrl+LMB" on reel
//! in, Ctrl+click reels and doesn't also fire. A modifier is only taken by
//! chords that are actually bound, so holding Shift to sprint while
//! clicking still fires.
//!
//! `keybinds.ron` maps action names to lists of bindings, e.g.
//! `{ "ReelIn": ["Mouse4", "Ctrl+LMB"], "Interact": ["F"] }`. Actions it
//! leaves out keep their defaults; an empty list unbinds one.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const KEYBINDS_PATH: &str = "keybinds.ron";

/// Bindings per action.
const SLOTS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,
    RollLeft,
    RollRight,
    Sprint,
    Brake,
    Jump,
    ToggleBoots,
    ToggleAssist,
    SetReference,
    Interact,
    Grapple,
    ReelIn,
    PayOut,
    TuningPanel,
    ReleaseMouse,
    DebugLurch,
}

impl Action {
    pub const ALL: [Action; 21] = [
        Action::Forward,
        Action::Back,
        Action::Left,
        Action::Right,
        Action::Up,
        Action::Down,
        Action::RollLeft,
        Action::RollRight,
        Action::Sprint,
        Action::Brake,
        Action::Jump,
        Action::ToggleBoots,
        Action::ToggleAssist,
        Action::SetReference,
        Action::Interact,
        Action::Grapple,
        Action::ReelIn,
        Action::PayOut,
        Action::TuningPanel,
        Action::ReleaseMouse,
        Action::DebugLurch,
    ];

    fn defaults(self) -> &'static [&'static str] {
        match self {
            Action::Forward => &["W"],
            Action::Back => &["S"],
            Action::Left => &["A"],
            Action::Right => &["D"],
            Action::Up => &["Space"],
            Action::Down => &["C"],
            Action::RollLeft => &["Q"],
            Action::RollRight => &["E"],
            Action::Sprint => &["Shift"],
            Action::Brake => &["X"],
            Action::Jump => &["Space"],
            Action::ToggleBoots => &["B"],
            Action::ToggleAssist => &["V"],
            Action::SetReference => &["T"],
            Action::Interact => &["F"],
            Action::Grapple => &["LMB"],
            Action::ReelIn => &["Mouse4", "Ctrl+LMB"],
            Action::PayOut => &["Mouse5", "Alt+LMB"],
            Action::TuningPanel => &["F1"],
            Action::ReleaseMouse => &["Escape"],
            Action::DebugLurch => &["L"],
        }
    }
}

/// A key or mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Key(KeyCode),
    Mouse(MouseButton),
}

/// Keys that can be held as part of a chord. Left and right count alike.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
    Tab,
    CapsLock,
}

impl Modifier {
    const ALL: [Modifier; 5] = [
        Modifier::Shift,
        Modifier::Ctrl,
        Modifier::Alt,
        Modifier::Tab,
        Modifier::CapsLock,
    ];

    fn keys(self) -> &'static [KeyCode] {
        match self {
            Modifier::Shift => &[KeyCode::ShiftLeft, KeyCode::ShiftRight],
            Modifier::Ctrl => &[KeyCode::ControlLeft, KeyCode::ControlRight],
            Modifier::Alt => &[KeyCode::AltLeft, KeyCode::AltRight],
            Modifier::Tab => &[KeyCode::Tab],
            Modifier::CapsLock => &[KeyCode::CapsLock],
        }
    }

    fn name(self) -> &'static str {
        match self {
            Modifier::Shift => "Shift",
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
            Modifier::Tab => "Tab",
            Modifier::CapsLock => "CapsLock",
        }
    }

    fn from_name(name: &str) -> Option<Modifier> {
        Modifier::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(name))
    }

    /// The modifier this button is, if it is one.
    fn of(button: Button) -> Option<Modifier> {
        let Button::Key(key) = button else {
            return None;
        };
        Modifier::ALL.into_iter().find(|m| m.keys().contains(&key))
    }
}

/// A button and the modifiers held with it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    /// Sorted, without duplicates.
    pub modifiers: Vec<Modifier>,
    pub button: Button,
}

/// Names for buttons, both ways. Letters, digits and F-keys are generated.
const NAMED: &[(&str, Button)] = &[
    ("LMB", Button::Mouse(MouseButton::Left)),
    ("RMB", Button::Mouse(MouseButton::Right)),
    ("MMB", Button::Mouse(MouseButton::Middle)),
    ("Mouse4", Button::Mouse(MouseButton::Back)),
    ("Mouse5", Button::Mouse(MouseButton::Forward)),
    ("Space", Button::Key(KeyCode::Space)),
    ("Escape", Button::Key(KeyCode::Escape)),
    ("Enter", Button::Key(KeyCode::Enter)),
    ("Backspace", Button::Key(KeyCode::Backspace)),
    ("Backquote", Button::Key(KeyCode::Backquote)),
    ("Up", Button::Key(KeyCode::ArrowUp)),
    ("Down", Button::Key(KeyCode::ArrowDown)),
    ("Left", Button::Key(KeyCode::ArrowLeft)),
    ("Right", Button::Key(KeyCode::ArrowRight)),
    // Modifiers as plain buttons (e.g. Shift to sprint).
    ("Shift", Button::Key(KeyCode::ShiftLeft)),
    ("RShift", Button::Key(KeyCode::ShiftRight)),
    ("Ctrl", Button::Key(KeyCode::ControlLeft)),
    ("RCtrl", Button::Key(KeyCode::ControlRight)),
    ("Alt", Button::Key(KeyCode::AltLeft)),
    ("RAlt", Button::Key(KeyCode::AltRight)),
    ("Tab", Button::Key(KeyCode::Tab)),
    ("CapsLock", Button::Key(KeyCode::CapsLock)),
];

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF,
    KeyCode::KeyG, KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL,
    KeyCode::KeyM, KeyCode::KeyN, KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR,
    KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU, KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX,
    KeyCode::KeyY, KeyCode::KeyZ,
];
const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4,
    KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
];
const F_KEYS: [KeyCode; 12] = [
    KeyCode::F1, KeyCode::F2, KeyCode::F3, KeyCode::F4, KeyCode::F5, KeyCode::F6,
    KeyCode::F7, KeyCode::F8, KeyCode::F9, KeyCode::F10, KeyCode::F11, KeyCode::F12,
];

fn button_from_name(name: &str) -> Option<Button> {
    if let Some((_, button)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(*button);
    }
    let upper = name.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    match bytes {
        [c @ b'A'..=b'Z'] => Some(Button::Key(LETTERS[(c - b'A') as usize])),
        [c @ b'0'..=b'9'] => Some(Button::Key(DIGITS[(c - b'0') as usize])),
        [b'F', ..] => {
            let n: usize = upper[1..].parse().ok()?;
            F_KEYS.get(n.checked_sub(1)?).map(|k| Button::Key(*k))
        }
        _ => None,
    }
}

fn button_name(button: Button) -> String {
    if let Some((name, _)) = NAMED.iter().find(|(_, b)| *b == button) {
        return name.to_string();
    }
    let Button::Key(key) = button else {
        return format!("{button:?}");
    };
    let find = |keys: &[KeyCode]| keys.iter().position(|k| *k == key);
    if let Some(i) = find(&LETTERS) {
        return char::from(b'A' + i as u8).to_string();
    }
    if let Some(i) = find(&DIGITS) {
        return i.to_string();
    }
    if let Some(i) = find(&F_KEYS) {
        return format!("F{}", i + 1);
    }
    format!("{key:?}")
}

impl std::str::FromStr for Binding {
    type Err = String;

    /// "Ctrl+Alt+LMB": modifiers, then the button, joined by '+'.
    fn from_str(text: &str) -> Result<Binding, String> {
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (button, modifiers) = parts.split_last().ok_or("empty binding")?;
        let button = button_from_name(button).ok_or_else(|| format!("unknown button {button:?}"))?;
        let mut modifiers = modifiers
            .iter()
            .map(|m| Modifier::from_name(m).ok_or_else(|| format!("unknown modifier {m:?}")))
            .collect::<Result<Vec<_>, _>>()?;
        modifiers.sort();
        modifiers.dedup();
        if Modifier::of(button).is_some_and(|own| modifiers.contains(&own)) {
            return Err(format!("{text:?} holds its own button as a modifier"));
        }
        Ok(Binding { modifiers, button })
    }
}

impl std::fmt::Display for Binding {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for m in &self.modifiers {
            write!(f, "{}+", m.name())?;
        }
        f.write_str(&button_name(self.button))
    }
}

/// Every action's bindings.
#[derive(Resource, Clone, Debug)]
pub struct Bindings(HashMap<Action, Vec<Binding>>);

impl Default for Bindings {
    fn default() -> Bindings {
        Bindings(
            Action::ALL
                .into_iter()
                .map(|action| {
                    let bindings = action.defaults().iter().map(|b| b.parse().expect("default binding")).collect();
                    (action, bindings)
                })
                .collect(),
        )
    }
}

impl Bindings {
    /// The defaults, overridden by whatever `text` (the contents of
    /// `keybinds.ron`) sets. Bad entries are reported and skipped.
    pub fn from_ron(text: &str) -> (Bindings, Vec<String>) {
        let mut bindings = Bindings::default();
        let mut problems = Vec::new();
        let overrides: HashMap<String, Vec<String>> = match ron::from_str(text) {
            Ok(map) => map,
            Err(e) => return (bindings, vec![format!("{KEYBINDS_PATH}: {e}")]),
        };
        for (name, texts) in overrides {
            let Some(action) = Action::ALL.into_iter().find(|a| format!("{a:?}") == name) else {
                problems.push(format!("{KEYBINDS_PATH}: unknown action {name:?}"));
                continue;
            };
            if texts.len() > SLOTS {
                problems.push(format!("{KEYBINDS_PATH}: {name} has more than {SLOTS} bindings; using the first {SLOTS}"));
            }
            let mut parsed = Vec::new();
            for text in texts.iter().take(SLOTS) {
                match text.parse() {
                    Ok(binding) => parsed.push(binding),
                    Err(e) => problems.push(format!("{KEYBINDS_PATH}: {name}: {e}")),
                }
            }
            bindings.0.insert(action, parsed);
        }
        (bindings, problems)
    }

    /// The first binding of `action`, as text for a prompt.
    pub fn label(&self, action: Action) -> String {
        self.0
            .get(&action)
            .and_then(|b| b.first())
            .map_or_else(|| "unbound".into(), Binding::to_string)
    }
}

/// What's held this frame, to evaluate bindings against.
pub struct Held<'a> {
    pub keys: &'a ButtonInput<KeyCode>,
    pub mouse: &'a ButtonInput<MouseButton>,
}

impl Held<'_> {
    fn pressed(&self, button: Button) -> bool {
        match button {
            Button::Key(k) => self.keys.pressed(k),
            Button::Mouse(m) => self.mouse.pressed(m),
        }
    }

    fn just_pressed(&self, button: Button) -> bool {
        match button {
            Button::Key(k) => self.keys.just_pressed(k),
            Button::Mouse(m) => self.mouse.just_pressed(m),
        }
    }

    fn modifier(&self, modifier: Modifier) -> bool {
        modifier.keys().iter().any(|k| self.keys.pressed(*k))
    }

    /// Whether `binding`'s modifiers are held and no more specific chord on
    /// the same button is.
    fn chord_wins(&self, bindings: &Bindings, binding: &Binding) -> bool {
        let holds = |b: &Binding| b.modifiers.iter().all(|m| self.modifier(*m));
        holds(binding)
            && !bindings.0.values().flatten().any(|other| {
                other.button == binding.button
                    && other.modifiers.len() > binding.modifiers.len()
                    && binding.modifiers.iter().all(|m| other.modifiers.contains(m))
                    && holds(other)
            })
    }

    /// Whether `action` is held.
    pub fn active(&self, bindings: &Bindings, action: Action) -> bool {
        bindings.0.get(&action).is_some_and(|list| {
            list.iter().any(|b| self.pressed(b.button) && self.chord_wins(bindings, b))
        })
    }

    /// Whether `action` was pressed this frame.
    pub fn started(&self, bindings: &Bindings, action: Action) -> bool {
        bindings.0.get(&action).is_some_and(|list| {
            list.iter().any(|b| self.just_pressed(b.button) && self.chord_wins(bindings, b))
        })
    }
}

/// Loads `keybinds.ron` if there is one.
pub fn load_bindings(mut commands: Commands) {
    let bindings = match std::fs::read_to_string(KEYBINDS_PATH) {
        Ok(text) => {
            let (bindings, problems) = Bindings::from_ron(&text);
            for problem in problems {
                warn!("{problem}");
            }
            info!("key bindings from {KEYBINDS_PATH}");
            bindings
        }
        Err(_) => Bindings::default(),
    };
    commands.insert_resource(bindings);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(keys: &[KeyCode], mouse: &[MouseButton]) -> (ButtonInput<KeyCode>, ButtonInput<MouseButton>) {
        let mut k = ButtonInput::default();
        let mut m = ButtonInput::default();
        keys.iter().for_each(|key| k.press(*key));
        mouse.iter().for_each(|b| m.press(*b));
        (k, m)
    }

    #[test]
    fn bindings_read_and_print_the_same() {
        for text in ["LMB", "Ctrl+LMB", "Alt+Ctrl+Mouse4", "F12", "Shift", "CapsLock+Q", "7"] {
            let binding: Binding = text.parse().unwrap();
            let again: Binding = binding.to_string().parse().unwrap();
            assert_eq!(binding, again, "{text}");
        }
        assert_eq!("alt+ctrl+mouse4".parse::<Binding>().unwrap().to_string(), "Ctrl+Alt+Mouse4");
        assert!("Ctrl+Ctrl".parse::<Binding>().is_err());
        assert!("Hyper+A".parse::<Binding>().is_err());
    }

    #[test]
    fn the_most_specific_chord_wins() {
        let bindings = Bindings::default();
        let (keys, mouse) = held(&[KeyCode::ControlLeft], &[MouseButton::Left]);
        let h = Held { keys: &keys, mouse: &mouse };
        assert!(h.active(&bindings, Action::ReelIn));
        assert!(!h.active(&bindings, Action::Grapple));
        let (keys, mouse) = held(&[], &[MouseButton::Left]);
        let h = Held { keys: &keys, mouse: &mouse };
        assert!(h.active(&bindings, Action::Grapple));
        assert!(!h.active(&bindings, Action::ReelIn));
    }

    #[test]
    fn unbound_modifiers_are_ignored() {
        // Sprinting while clicking still grapples: Shift+LMB isn't bound.
        let bindings = Bindings::default();
        let (keys, mouse) = held(&[KeyCode::ShiftLeft], &[MouseButton::Left]);
        let h = Held { keys: &keys, mouse: &mouse };
        assert!(h.active(&bindings, Action::Grapple));
        assert!(h.active(&bindings, Action::Sprint));
    }

    #[test]
    fn the_file_overrides_some_actions() {
        let (bindings, problems) =
            Bindings::from_ron(r#"{ "ReelIn": ["RMB"], "Interact": [], "Bogus": ["A"], "Jump": ["Nope"] }"#);
        assert_eq!(bindings.label(Action::ReelIn), "RMB");
        assert_eq!(bindings.label(Action::Interact), "unbound");
        assert_eq!(bindings.label(Action::Grapple), "LMB");
        assert_eq!(problems.len(), 2, "{problems:?}");
    }
}
