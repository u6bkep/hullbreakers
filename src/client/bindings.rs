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
pub const SLOTS: usize = 2;

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
    /// Put down what's in hand (cargo, else the main hand's gun).
    DropItem,
    /// The main hand's item (primary) and the off hand's (secondary).
    MainUse,
    OffUse,
    /// Each hand's alternate functions (for the grapple: reel in, pay out).
    MainAlt,
    OffAlt,
    MainAlt2,
    OffAlt2,
    /// Reel in and pay out, whichever hand holds the grapple.
    ReelIn,
    PayOut,
    Reload,
    ReloadMain,
    ReloadOff,
    Stance1,
    Stance2,
    Stance3,
    Stance4,
    /// The menu, open on the inventory.
    Loadout,
    TuningPanel,
    Menu,
    DebugLurch,
}

impl Action {
    pub const ALL: [Action; 35] = [
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
        Action::DropItem,
        Action::MainUse,
        Action::OffUse,
        Action::MainAlt,
        Action::OffAlt,
        Action::MainAlt2,
        Action::OffAlt2,
        Action::ReelIn,
        Action::PayOut,
        Action::Reload,
        Action::ReloadMain,
        Action::ReloadOff,
        Action::Stance1,
        Action::Stance2,
        Action::Stance3,
        Action::Stance4,
        Action::Loadout,
        Action::TuningPanel,
        Action::Menu,
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
            Action::DropItem => &["G"],
            Action::MainUse => &["LMB"],
            Action::OffUse => &["RMB"],
            Action::MainAlt => &["Ctrl+LMB"],
            Action::OffAlt => &["Ctrl+RMB"],
            Action::MainAlt2 => &["Alt+LMB"],
            Action::OffAlt2 => &["Alt+RMB"],
            Action::ReelIn => &["Mouse4"],
            Action::PayOut => &["Mouse5"],
            Action::Reload => &["R"],
            Action::ReloadMain => &[],
            Action::ReloadOff => &[],
            Action::Stance1 => &["1"],
            Action::Stance2 => &["2"],
            Action::Stance3 => &["3"],
            Action::Stance4 => &["4"],
            Action::Loadout => &["I"],
            Action::TuningPanel => &["F1"],
            Action::Menu => &["Escape"],
            Action::DebugLurch => &["L"],
        }
    }

    /// The default bindings.
    pub fn default_bindings(self) -> Vec<Binding> {
        self.defaults().iter().map(|b| b.parse().expect("default binding")).collect()
    }

    /// What it's called in the controls menu.
    pub fn name(self) -> &'static str {
        match self {
            Action::Forward => "Forward",
            Action::Back => "Back",
            Action::Left => "Left",
            Action::Right => "Right",
            Action::Up => "Thrust up",
            Action::Down => "Thrust down",
            Action::RollLeft => "Roll left",
            Action::RollRight => "Roll right",
            Action::Sprint => "Sprint / boost",
            Action::Brake => "Brake",
            Action::Jump => "Jump",
            Action::ToggleBoots => "Mag boots",
            Action::ToggleAssist => "Flight assist",
            Action::SetReference => "Set reference body",
            Action::Interact => "Interact",
            Action::DropItem => "Drop held item",
            Action::MainUse => "Main hand: use",
            Action::OffUse => "Off hand: use",
            Action::MainAlt => "Main hand: alternate",
            Action::OffAlt => "Off hand: alternate",
            Action::MainAlt2 => "Main hand: second alternate",
            Action::OffAlt2 => "Off hand: second alternate",
            Action::ReelIn => "Reel in",
            Action::PayOut => "Pay out",
            Action::Reload => "Reload",
            Action::ReloadMain => "Reload main hand",
            Action::ReloadOff => "Reload off hand",
            Action::Stance1 => "Stance 1",
            Action::Stance2 => "Stance 2",
            Action::Stance3 => "Stance 3",
            Action::Stance4 => "Stance 4",
            Action::Loadout => "Inventory",
            Action::TuningPanel => "Tuning panel",
            Action::Menu => "Menu",
            Action::DebugLurch => "Debug: ship lurch",
        }
    }

    /// The heading it's listed under in the controls menu.
    pub fn group(self) -> &'static str {
        match self {
            Action::Forward
            | Action::Back
            | Action::Left
            | Action::Right
            | Action::Up
            | Action::Down
            | Action::RollLeft
            | Action::RollRight
            | Action::Sprint
            | Action::Brake
            | Action::Jump
            | Action::ToggleBoots
            | Action::ToggleAssist
            | Action::SetReference => "Movement",
            Action::Interact
            | Action::DropItem
            | Action::MainUse
            | Action::OffUse
            | Action::MainAlt
            | Action::OffAlt
            | Action::MainAlt2
            | Action::OffAlt2
            | Action::Reload
            | Action::ReloadMain
            | Action::ReloadOff => "Hands",
            Action::ReelIn | Action::PayOut => "Grapple",
            Action::Stance1 | Action::Stance2 | Action::Stance3 | Action::Stance4 => "Stances",
            Action::Loadout | Action::TuningPanel | Action::Menu | Action::DebugLurch => "Interface",
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
    pub const ALL: [Modifier; 5] = [
        Modifier::Shift,
        Modifier::Ctrl,
        Modifier::Alt,
        Modifier::Tab,
        Modifier::CapsLock,
    ];

    pub fn keys(self) -> &'static [KeyCode] {
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
    pub fn of(button: Button) -> Option<Modifier> {
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
    ("Minus", Button::Key(KeyCode::Minus)),
    ("Equal", Button::Key(KeyCode::Equal)),
    ("BracketLeft", Button::Key(KeyCode::BracketLeft)),
    ("BracketRight", Button::Key(KeyCode::BracketRight)),
    ("Backslash", Button::Key(KeyCode::Backslash)),
    ("Semicolon", Button::Key(KeyCode::Semicolon)),
    ("Quote", Button::Key(KeyCode::Quote)),
    ("Comma", Button::Key(KeyCode::Comma)),
    ("Period", Button::Key(KeyCode::Period)),
    ("Slash", Button::Key(KeyCode::Slash)),
    ("Insert", Button::Key(KeyCode::Insert)),
    ("Delete", Button::Key(KeyCode::Delete)),
    ("Home", Button::Key(KeyCode::Home)),
    ("End", Button::Key(KeyCode::End)),
    ("PageUp", Button::Key(KeyCode::PageUp)),
    ("PageDown", Button::Key(KeyCode::PageDown)),
    ("Num0", Button::Key(KeyCode::Numpad0)),
    ("Num1", Button::Key(KeyCode::Numpad1)),
    ("Num2", Button::Key(KeyCode::Numpad2)),
    ("Num3", Button::Key(KeyCode::Numpad3)),
    ("Num4", Button::Key(KeyCode::Numpad4)),
    ("Num5", Button::Key(KeyCode::Numpad5)),
    ("Num6", Button::Key(KeyCode::Numpad6)),
    ("Num7", Button::Key(KeyCode::Numpad7)),
    ("Num8", Button::Key(KeyCode::Numpad8)),
    ("Num9", Button::Key(KeyCode::Numpad9)),
    ("NumEnter", Button::Key(KeyCode::NumpadEnter)),
    ("NumPlus", Button::Key(KeyCode::NumpadAdd)),
    ("NumMinus", Button::Key(KeyCode::NumpadSubtract)),
    ("NumStar", Button::Key(KeyCode::NumpadMultiply)),
    ("NumSlash", Button::Key(KeyCode::NumpadDivide)),
    ("NumPeriod", Button::Key(KeyCode::NumpadDecimal)),
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
        Bindings(Action::ALL.into_iter().map(|a| (a, a.default_bindings())).collect())
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

    /// The file that gives these bindings: whatever differs from the
    /// defaults.
    pub fn to_ron(&self) -> String {
        let mut changed: Vec<(String, Vec<String>)> = Action::ALL
            .into_iter()
            .filter(|a| self.get(*a) != a.default_bindings().as_slice())
            .map(|a| (format!("{a:?}"), self.get(a).iter().map(Binding::to_string).collect()))
            .collect();
        changed.sort();
        let map: std::collections::BTreeMap<_, _> = changed.into_iter().collect();
        ron::ser::to_string_pretty(&map, ron::ser::PrettyConfig::default()).expect("bindings serialize")
    }

    /// Writes `keybinds.ron`.
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::write(KEYBINDS_PATH, self.to_ron())
    }

    pub fn get(&self, action: Action) -> &[Binding] {
        self.0.get(&action).map_or(&[], Vec::as_slice)
    }

    /// Sets (or, with `None`, clears) one of `action`'s two bindings.
    pub fn set(&mut self, action: Action, slot: usize, binding: Option<Binding>) {
        let list = self.0.entry(action).or_default();
        match binding {
            Some(b) if slot < list.len() => list[slot] = b,
            Some(b) if slot < SLOTS => list.push(b),
            Some(_) => {}
            None if slot < list.len() => {
                list.remove(slot);
            }
            None => {}
        }
        // The same binding twice is one.
        let mut seen = Vec::new();
        list.retain(|b| {
            let new = !seen.contains(b);
            seen.push(b.clone());
            new
        });
    }

    /// The other actions bound to exactly `binding`.
    pub fn sharing(&self, action: Action, binding: &Binding) -> Vec<Action> {
        Action::ALL
            .into_iter()
            .filter(|a| *a != action && self.get(*a).contains(binding))
            .collect()
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
        assert!(h.active(&bindings, Action::MainAlt));
        assert!(!h.active(&bindings, Action::MainUse));
        let (keys, mouse) = held(&[], &[MouseButton::Left]);
        let h = Held { keys: &keys, mouse: &mouse };
        assert!(h.active(&bindings, Action::MainUse));
        assert!(!h.active(&bindings, Action::MainAlt));
    }

    #[test]
    fn unbound_modifiers_are_ignored() {
        // Sprinting while clicking still fires: Shift+LMB isn't bound.
        let bindings = Bindings::default();
        let (keys, mouse) = held(&[KeyCode::ShiftLeft], &[MouseButton::Left]);
        let h = Held { keys: &keys, mouse: &mouse };
        assert!(h.active(&bindings, Action::MainUse));
        assert!(h.active(&bindings, Action::Sprint));
    }

    #[test]
    fn every_nameable_button_reads_back() {
        for (name, button) in NAMED {
            let binding: Binding = name.parse().unwrap();
            assert_eq!(binding.button, *button, "{name}");
        }
    }

    #[test]
    fn saved_bindings_read_back_and_only_hold_changes() {
        let mut bindings = Bindings::default();
        bindings.set(Action::ReelIn, 1, Some("Ctrl+Q".parse().unwrap()));
        bindings.set(Action::Interact, 0, None);
        let text = bindings.to_ron();
        assert!(!text.contains("Forward"), "{text}");
        let (again, problems) = Bindings::from_ron(&text);
        assert!(problems.is_empty(), "{problems:?}");
        for action in Action::ALL {
            assert_eq!(again.get(action), bindings.get(action), "{action:?}");
        }
        assert_eq!(again.get(Action::ReelIn).len(), 2);
        assert!(again.get(Action::Interact).is_empty());
    }

    #[test]
    fn the_file_overrides_some_actions() {
        let (bindings, problems) =
            Bindings::from_ron(r#"{ "ReelIn": ["RMB"], "Interact": [], "Bogus": ["A"], "Jump": ["Nope"] }"#);
        assert_eq!(bindings.label(Action::ReelIn), "RMB");
        assert_eq!(bindings.label(Action::Interact), "unbound");
        assert_eq!(bindings.label(Action::MainUse), "LMB");
        assert_eq!(problems.len(), 2, "{problems:?}");
    }
}
