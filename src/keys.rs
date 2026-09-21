//! evdev キーコードと Emacs 変換表。Windows 版の EmacsBindings.cs に対応する。

use evdev::KeyCode;

pub const LEFTCTRL: u16 = KeyCode::KEY_LEFTCTRL.code();
pub const RIGHTCTRL: u16 = KeyCode::KEY_RIGHTCTRL.code();
pub const LEFTALT: u16 = KeyCode::KEY_LEFTALT.code();
pub const RIGHTALT: u16 = KeyCode::KEY_RIGHTALT.code();
pub const LEFTSHIFT: u16 = KeyCode::KEY_LEFTSHIFT.code();
pub const RIGHTSHIFT: u16 = KeyCode::KEY_RIGHTSHIFT.code();
pub const LEFTMETA: u16 = KeyCode::KEY_LEFTMETA.code();
pub const RIGHTMETA: u16 = KeyCode::KEY_RIGHTMETA.code();
pub const CAPSLOCK: u16 = KeyCode::KEY_CAPSLOCK.code();

pub const LEFT: u16 = KeyCode::KEY_LEFT.code();
pub const RIGHT: u16 = KeyCode::KEY_RIGHT.code();
pub const UP: u16 = KeyCode::KEY_UP.code();
pub const DOWN: u16 = KeyCode::KEY_DOWN.code();
pub const HOME: u16 = KeyCode::KEY_HOME.code();
pub const END: u16 = KeyCode::KEY_END.code();
pub const DELETE: u16 = KeyCode::KEY_DELETE.code();

pub const KEY_A: u16 = KeyCode::KEY_A.code();
pub const KEY_B: u16 = KeyCode::KEY_B.code();
pub const KEY_D: u16 = KeyCode::KEY_D.code();
pub const KEY_E: u16 = KeyCode::KEY_E.code();
pub const KEY_F: u16 = KeyCode::KEY_F.code();
pub const KEY_K: u16 = KeyCode::KEY_K.code();
pub const KEY_N: u16 = KeyCode::KEY_N.code();
pub const KEY_P: u16 = KeyCode::KEY_P.code();
pub const COMMA: u16 = KeyCode::KEY_COMMA.code();
pub const DOT: u16 = KeyCode::KEY_DOT.code();

pub fn is_alt(code: u16) -> bool {
    code == LEFTALT || code == RIGHTALT
}

pub fn is_ctrl(code: u16) -> bool {
    code == LEFTCTRL || code == RIGHTCTRL
}

pub fn is_shift(code: u16) -> bool {
    code == LEFTSHIFT || code == RIGHTSHIFT
}

/// 左右どちらの修飾キーが物理的に押されているか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sides {
    pub left: bool,
    pub right: bool,
}

impl Sides {
    pub fn any(self) -> bool {
        self.left || self.right
    }
}

/// 設定側の「Ctrlの対象」「Altの対象」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SideSelection {
    #[default]
    Both,
    Left,
    Right,
}

impl SideSelection {
    pub fn allows(self, down: Sides) -> bool {
        match self {
            SideSelection::Left => down.left,
            SideSelection::Right => down.right,
            SideSelection::Both => down.any(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers { control: false, alt: false, shift: false, meta: false };
    pub const CONTROL: Modifiers = Modifiers { control: true, ..Modifiers::NONE };
    pub const ALT: Modifiers = Modifiers { alt: true, ..Modifiers::NONE };
    pub const ALT_SHIFT: Modifiers = Modifiers { alt: true, shift: true, ..Modifiers::NONE };

    pub fn is_none(self) -> bool {
        self == Modifiers::NONE
    }
}

/// 13 種類の Emacs ショートカット。設定キー名と 1 対 1 で対応する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shortcut {
    ControlB,
    ControlF,
    ControlP,
    ControlN,
    ControlA,
    ControlE,
    AltB,
    AltF,
    AltLess,
    AltGreater,
    ControlD,
    AltD,
    ControlK,
}

/// 変換後に送るキー列の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    /// 目的キーを 1 打鍵送る。
    Key,
    /// 行末まで削除: Shift+End で選択してから Delete。
    DeleteToLineEnd,
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub shortcut: Shortcut,
    pub label: &'static str,
    pub source: u16,
    pub modifiers: Modifiers,
    pub target: u16,
    pub target_control: bool,
    pub action: InputAction,
}

const fn key(
    shortcut: Shortcut,
    label: &'static str,
    source: u16,
    modifiers: Modifiers,
    target: u16,
    target_control: bool,
) -> Binding {
    Binding { shortcut, label, source, modifiers, target, target_control, action: InputAction::Key }
}

pub fn bindings() -> &'static [Binding] {
    &BINDINGS
}

static BINDINGS: [Binding; 13] = [
    key(Shortcut::ControlB, "Ctrl+B：左へ移動", KEY_B, Modifiers::CONTROL, LEFT, false),
    key(Shortcut::ControlF, "Ctrl+F：右へ移動", KEY_F, Modifiers::CONTROL, RIGHT, false),
    key(Shortcut::ControlP, "Ctrl+P：上へ移動", KEY_P, Modifiers::CONTROL, UP, false),
    key(Shortcut::ControlN, "Ctrl+N：下へ移動", KEY_N, Modifiers::CONTROL, DOWN, false),
    key(Shortcut::ControlA, "Ctrl+A：行頭へ移動", KEY_A, Modifiers::CONTROL, HOME, false),
    key(Shortcut::ControlE, "Ctrl+E：行末へ移動", KEY_E, Modifiers::CONTROL, END, false),
    key(Shortcut::AltB, "Alt+B：前の単語へ移動", KEY_B, Modifiers::ALT, LEFT, true),
    key(Shortcut::AltF, "Alt+F：次の単語へ移動", KEY_F, Modifiers::ALT, RIGHT, true),
    key(Shortcut::AltLess, "Alt+<：文書の先頭へ移動", COMMA, Modifiers::ALT_SHIFT, HOME, true),
    key(Shortcut::AltGreater, "Alt+>：文書の末尾へ移動", DOT, Modifiers::ALT_SHIFT, END, true),
    key(Shortcut::ControlD, "Ctrl+D：カーソル位置の文字を削除", KEY_D, Modifiers::CONTROL, DELETE, false),
    key(Shortcut::AltD, "Alt+D：次の単語を削除", KEY_D, Modifiers::ALT, DELETE, true),
    Binding {
        shortcut: Shortcut::ControlK,
        label: "Ctrl+K：行末まで削除（行末では改行を削除）",
        source: KEY_K,
        modifiers: Modifiers::CONTROL,
        target: DELETE,
        target_control: false,
        action: InputAction::DeleteToLineEnd,
    },
];

/// 変換対象になりうるキーか。対象外のキーは状態を持たずそのまま通す。
pub fn is_source_key(code: u16) -> bool {
    matches!(code, KEY_A | KEY_B | KEY_D | KEY_E | KEY_F | KEY_K | KEY_N | KEY_P | COMMA | DOT)
}
