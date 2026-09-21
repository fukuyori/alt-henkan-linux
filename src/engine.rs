//! 入力変換の状態機械。OS 呼び出しもディスク I/O も行わない純粋なロジックで、
//! Windows 版の AltInputService.cs / EmacsKeyboardState.cs に対応する。
//!
//! Windows 版はフックが「握り潰す／通す」を返す形だったが、evdev では入力デバイスを
//! EVIOCGRAB で占有し、通すべきイベントだけ uinput へ書き戻す。よって「通す」は
//! `Action::Key` の出力そのものになる。

use std::collections::{HashMap, HashSet};

use crate::config::{Config, Shortcuts};
use crate::keys::{self, Binding, InputAction, Modifiers, Shortcut, SideSelection, Sides};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImeCommand {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// uinput へ書き出すキーイベント。value は 0=離す 1=押す 2=オートリピート。
    Key { code: u16, value: i32 },
    Ime(ImeCommand),
    ModeChanged { emacs_active: bool },
    /// 実行した Emacs 操作の名前。診断ログ用で、入力には影響しない。
    Emacs(&'static str),
}

impl Action {
    fn press(code: u16) -> Action {
        Action::Key { code, value: 1 }
    }

    fn release(code: u16) -> Action {
        Action::Key { code, value: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltSide {
    Left,
    Right,
}

impl AltSide {
    fn index(self) -> usize {
        match self {
            AltSide::Left => 0,
            AltSide::Right => 1,
        }
    }

    fn code(self) -> u16 {
        match self {
            AltSide::Left => keys::LEFTALT,
            AltSide::Right => keys::RIGHTALT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtrlSide {
    Left,
    Right,
}

impl CtrlSide {
    fn index(self) -> usize {
        match self {
            CtrlSide::Left => 0,
            CtrlSide::Right => 1,
        }
    }

    fn code(self) -> u16 {
        match self {
            CtrlSide::Left => keys::LEFTCTRL,
            CtrlSide::Right => keys::RIGHTCTRL,
        }
    }
}

/// 保留中の修飾キーの状態。押下を握ったまま、用途が決まるまで送出を遅らせる。
#[derive(Debug, Clone, Copy, Default)]
struct ModifierState {
    is_down: bool,
    /// 押下をそのまま通した (もはや保留していない)。
    original_down_passed: bool,
    /// 保留を解いて本来の修飾キーの押下を送った。
    native_down_sent: bool,
    /// Emacs 変換に使われたので、離したときに何も送らない。
    consumed_by_emacs: bool,
    /// Alt+< / Alt+> のために Shift 付きで保留を続けている。
    deferred_shift_chord: bool,
    down_at_ms: u64,
}

impl ModifierState {
    fn start(&mut self, now_ms: u64) {
        *self = ModifierState { is_down: true, down_at_ms: now_ms, ..ModifierState::default() };
    }

    fn reset(&mut self) {
        *self = ModifierState::default();
    }

    /// 押されているのに、まだ前面のアプリからは見えていない。
    fn hidden_from_foreground(&self) -> bool {
        self.is_down && !self.original_down_passed && !self.native_down_sent
    }
}

/// CapsLock と変換対象キーの捕捉状態。
#[derive(Debug, Default)]
struct EmacsState {
    active: bool,
    capslock_captured: bool,
    capslock_passed: bool,
    captured_keys: HashMap<u16, Shortcut>,
    passed_keys: HashSet<u16>,
}

impl EmacsState {
    /// CapsLock か変換対象キーを握っている最中か。
    fn gesture_active(&self) -> bool {
        self.capslock_captured || !self.captured_keys.is_empty()
    }

    /// 戻り値は (握り潰したか, モードが変わったか)。
    fn handle_capslock(&mut self, key_down: bool, enabled: bool) -> (bool, bool) {
        if !key_down {
            let captured = self.capslock_captured;
            self.capslock_captured = false;
            self.capslock_passed = false;
            return (captured, false);
        }
        if self.capslock_captured {
            return (true, false);
        }
        if self.capslock_passed {
            return (false, false);
        }
        if !enabled {
            self.capslock_passed = true;
            return (false, false);
        }
        self.capslock_captured = true;
        self.active = !self.active;
        (true, true)
    }

    /// 戻り値は (握り潰したか, 実行する変換)。押している最中に設定が変わっても
    /// 押下と離上の対応が崩れないよう、捕捉したキーは離すまで覚えておく。
    fn handle_key(
        &mut self,
        code: u16,
        key_down: bool,
        enabled: bool,
        modifiers: Modifiers,
        shortcuts: &Shortcuts,
        side_filter: Option<SideFilter>,
    ) -> (bool, Option<&'static Binding>) {
        if !keys::is_source_key(code) {
            return (false, None);
        }
        if !key_down {
            self.passed_keys.remove(&code);
            return (self.captured_keys.remove(&code).is_some(), None);
        }
        if self.passed_keys.contains(&code) {
            return (false, None);
        }
        let previously_captured = self.captured_keys.get(&code).copied();
        if enabled && self.active {
            let binding = resolve_binding(code, modifiers, shortcuts, side_filter);
            if let Some(binding) = binding
                && previously_captured.is_none_or(|shortcut| shortcut == binding.shortcut)
            {
                self.captured_keys.insert(code, binding.shortcut);
                return (true, Some(binding));
            }
        }
        if previously_captured.is_some() {
            return (true, None);
        }
        self.passed_keys.insert(code);
        (false, None)
    }

    /// 押しっぱなしの捕捉情報だけ捨てる。選択中のモードは保つ。
    fn reset_transient(&mut self) {
        self.capslock_captured = false;
        self.capslock_passed = false;
        self.captured_keys.clear();
        self.passed_keys.clear();
    }
}

/// 「Ctrlの対象」「Altの対象」の判定。
#[derive(Debug, Clone, Copy)]
pub struct SideFilter {
    control_down: Sides,
    alt_down: Sides,
    control_selection: SideSelection,
    alt_selection: SideSelection,
}

impl SideFilter {
    fn matches(&self, modifiers: Modifiers) -> bool {
        (!modifiers.control || self.control_selection.allows(self.control_down))
            && (!modifiers.alt || self.alt_selection.allows(self.alt_down))
    }
}

fn resolve_binding(
    code: u16,
    modifiers: Modifiers,
    shortcuts: &Shortcuts,
    side_filter: Option<SideFilter>,
) -> Option<&'static Binding> {
    keys::bindings().iter().find(|binding| {
        binding.source == code
            && binding.modifiers == modifiers
            && shortcuts.is_enabled(binding.shortcut)
            && side_filter.is_none_or(|filter| filter.matches(modifiers))
    })
}

/// 長押しして離した Alt を、本来の Alt として送るか IME 切り替えに使うか。
pub fn decide_alt_release(
    held_ms: u64,
    long_press_enabled: bool,
    long_press_ms: u64,
) -> AltReleaseAction {
    if long_press_enabled && held_ms >= long_press_ms {
        AltReleaseAction::NativeAlt
    } else {
        AltReleaseAction::Conversion
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AltReleaseAction {
    Conversion,
    NativeAlt,
}

pub struct Engine {
    config: Config,
    alt: [ModifierState; 2],
    ctrl: [ModifierState; 2],
    emacs: EmacsState,
    /// 物理的に押されているキー。evdev では入力を占有しているので実状態が分かる。
    down: HashSet<u16>,
}

impl Engine {
    pub fn new(config: Config) -> Self {
        let config = config.normalize();
        let emacs = EmacsState { active: config.initial_emacs_active(), ..EmacsState::default() };
        Engine {
            config,
            alt: [ModifierState::default(); 2],
            ctrl: [ModifierState::default(); 2],
            emacs,
            down: HashSet::new(),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn emacs_active(&self) -> bool {
        self.emacs.active
    }

    /// そのキーが物理的に押されているか。入力を占有しているので実状態が分かる。
    pub fn is_key_down(&self, code: u16) -> bool {
        self.down.contains(&code)
    }

    /// 設定を差し替える。Windows 版の UpdateSettings に対応する。
    pub fn update_config(&mut self, config: Config) -> Vec<Action> {
        let config = config.normalize();
        let mut actions = Vec::new();
        let was_enabled = self.config.enabled;
        let was_emacs_enabled = self.config.emacs_enabled();
        self.config = config;

        if !self.config.emacs_enabled() {
            if self.emacs.active {
                self.emacs.active = false;
                actions.push(Action::ModeChanged { emacs_active: false });
            }
        } else if !was_emacs_enabled {
            self.emacs.active = self.config.initial_emacs_active();
            actions.push(Action::ModeChanged { emacs_active: self.emacs.active });
        }

        if was_enabled && !self.config.enabled {
            actions.extend(self.release_everything());
        }
        actions
    }

    /// 現在のモードだけを変える。設定の Emacs 機能そのものは変えない。
    pub fn set_emacs_active(&mut self, active: bool) -> Vec<Action> {
        if !self.config.emacs_enabled() || self.emacs.active == active {
            return Vec::new();
        }
        self.emacs.active = active;
        vec![Action::ModeChanged { emacs_active: active }]
    }

    /// 保留中のキーを吐き出してから、握っている状態をすべて捨てる。
    pub fn release_everything(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        for index in 0..2 {
            if self.alt[index].native_down_sent || self.alt[index].original_down_passed {
                let code = if index == 0 { keys::LEFTALT } else { keys::RIGHTALT };
                actions.push(Action::release(code));
            }
            self.alt[index].reset();
            if self.ctrl[index].native_down_sent || self.ctrl[index].original_down_passed {
                let code = if index == 0 { keys::LEFTCTRL } else { keys::RIGHTCTRL };
                actions.push(Action::release(code));
            }
            self.ctrl[index].reset();
        }
        self.emacs.reset_transient();
        self.down.clear();
        actions
    }

    /// マウスのボタン・ホイール操作。保留中の Alt / Ctrl を本来の修飾キーに確定させる。
    pub fn on_pointer_activity(&mut self) -> Vec<Action> {
        if !self.config.enabled {
            return Vec::new();
        }
        let mut actions = self.promote_pending_ctrls();
        actions.extend(self.promote_pending_alts());
        actions
    }

    /// キーイベント 1 件を処理する。value は 0=離す 1=押す 2=オートリピート。
    pub fn on_key(&mut self, code: u16, value: i32, now_ms: u64) -> Vec<Action> {
        if value == 0 {
            self.down.remove(&code);
        } else {
            self.down.insert(code);
        }
        let key_down = value != 0;
        let emacs_enabled = self.config.emacs_enabled();
        let mut actions = Vec::new();

        if code == keys::CAPSLOCK {
            let (captured, mode_changed) = self.emacs.handle_capslock(key_down, emacs_enabled);
            if captured {
                self.consume_pending_alt_gestures();
                if mode_changed {
                    actions.push(Action::ModeChanged { emacs_active: self.emacs.active });
                }
                return actions;
            }
        } else if keys::is_ctrl(code) {
            let side = if code == keys::LEFTCTRL { CtrlSide::Left } else { CtrlSide::Right };
            return self.handle_ctrl(side, value, key_down);
        }

        let modifiers = if key_down && emacs_enabled && self.emacs.active && keys::is_source_key(code)
        {
            self.navigation_modifiers()
        } else {
            Modifiers::NONE
        };
        let side_filter = if modifiers.is_none() { None } else { Some(self.side_filter()) };
        let (captured, binding) = self.emacs.handle_key(
            code,
            key_down,
            emacs_enabled,
            modifiers,
            &self.config.emacs.shortcuts,
            side_filter,
        );
        if captured {
            if let Some(binding) = binding {
                if binding.modifiers.alt {
                    self.consume_pending_alt_gestures();
                }
                if binding.modifiers.control {
                    actions.extend(self.consume_pending_ctrl_gestures());
                }
                actions.push(Action::Emacs(binding.label));
                actions.extend(self.navigation_actions(binding));
            }
            return actions;
        }

        if !self.config.enabled {
            actions.push(Action::Key { code, value });
            return actions;
        }

        actions.extend(self.promote_pending_ctrls());

        if keys::is_alt(code) {
            let side = if code == keys::LEFTALT { AltSide::Left } else { AltSide::Right };
            actions.extend(self.handle_alt(side, value, key_down, now_ms));
            return actions;
        }

        if self.alt[0].is_down || self.alt[1].is_down {
            let shift_release_after_emacs = keys::is_shift(code)
                && !key_down
                && (self.alt[0].consumed_by_emacs || self.alt[1].consumed_by_emacs);
            if keys::is_shift(code) && key_down && self.can_defer_alt_shift() {
                // Alt+< / Alt+> は Shift を伴う。次のキーが来るまで Alt を隠したままにする。
                self.mark_deferred_shift_chord();
            } else if !shift_release_after_emacs {
                actions.extend(self.promote_pending_alts());
            }
        }

        actions.push(Action::Key { code, value });
        actions
    }

    fn handle_alt(
        &mut self,
        side: AltSide,
        value: i32,
        key_down: bool,
        now_ms: u64,
    ) -> Vec<Action> {
        let index = side.index();
        let code = side.code();
        if key_down {
            if self.alt[index].is_down {
                // オートリピート。通してあるものだけ通し続ける。
                return if self.alt[index].original_down_passed {
                    vec![Action::Key { code, value }]
                } else {
                    Vec::new()
                };
            }

            self.alt[index].start(now_ms);

            let other = 1 - index;
            if self.alt[other].is_down {
                let mut actions = self.promote_pending_alt(other);
                actions.extend(self.promote_pending_alt(index));
                return actions;
            }

            if self.any_non_alt_key_down() {
                let only_shift_down = self
                    .down
                    .iter()
                    .filter(|key| !keys::is_alt(**key))
                    .all(|key| keys::is_shift(*key));
                if self.shift_down()
                    && self.can_defer_alt_shift()
                    && !self.emacs.gesture_active()
                    && only_shift_down
                {
                    self.alt[index].deferred_shift_chord = true;
                } else {
                    self.alt[index].original_down_passed = true;
                    return vec![Action::press(code)];
                }
            }

            return Vec::new();
        }

        if !self.alt[index].is_down {
            return vec![Action::release(code)];
        }

        let held_ms = now_ms.saturating_sub(self.alt[index].down_at_ms);
        self.alt[index].is_down = false;

        if self.alt[index].original_down_passed {
            self.alt[index].reset();
            return vec![Action::release(code)];
        }

        if self.alt[index].native_down_sent {
            self.alt[index].reset();
            return vec![Action::release(code)];
        }

        if self.alt[index].consumed_by_emacs {
            self.alt[index].reset();
            return Vec::new();
        }

        if self.alt[index].deferred_shift_chord {
            // 角括弧のショートカットが続かなかったので、本来の Alt+Shift を保つ。
            self.alt[index].reset();
            return vec![Action::press(code), Action::release(code)];
        }

        let action = decide_alt_release(
            held_ms,
            self.config.long_press_enabled(side),
            self.config.long_press_ms,
        );
        self.alt[index].reset();
        match action {
            AltReleaseAction::NativeAlt => vec![Action::press(code), Action::release(code)],
            AltReleaseAction::Conversion => match side {
                AltSide::Left => vec![Action::Ime(ImeCommand::Off)],
                AltSide::Right => vec![Action::Ime(ImeCommand::On)],
            },
        }
    }

    fn handle_ctrl(&mut self, side: CtrlSide, value: i32, key_down: bool) -> Vec<Action> {
        let index = side.index();
        let code = side.code();
        if key_down {
            if self.ctrl[index].is_down {
                return if self.ctrl[index].original_down_passed {
                    vec![Action::Key { code, value }]
                } else {
                    Vec::new()
                };
            }

            self.ctrl[index].start(0);
            if !self.can_defer_ctrl(side) || self.any_other_input_down_for_ctrl() {
                self.ctrl[index].original_down_passed = true;
                return vec![Action::press(code)];
            }

            let other = 1 - index;
            if self.ctrl[other].is_down {
                let mut actions = self.promote_pending_ctrl(other);
                actions.extend(self.promote_pending_ctrl(index));
                return actions;
            }
            return Vec::new();
        }

        if !self.ctrl[index].is_down {
            return vec![Action::release(code)];
        }

        self.ctrl[index].is_down = false;
        if self.ctrl[index].original_down_passed || self.ctrl[index].native_down_sent {
            self.ctrl[index].reset();
            return vec![Action::release(code)];
        }

        let consumed = self.ctrl[index].consumed_by_emacs;
        self.ctrl[index].reset();
        if consumed {
            Vec::new()
        } else {
            // 単独で押して離した Ctrl は、本来の打鍵として送る。
            vec![Action::press(code), Action::release(code)]
        }
    }

    fn promote_pending_alts(&mut self) -> Vec<Action> {
        let mut actions = self.promote_pending_alt(0);
        actions.extend(self.promote_pending_alt(1));
        actions
    }

    fn promote_pending_alt(&mut self, index: usize) -> Vec<Action> {
        if !self.alt[index].hidden_from_foreground() {
            return Vec::new();
        }
        self.alt[index].native_down_sent = true;
        self.alt[index].deferred_shift_chord = false;
        vec![Action::press(if index == 0 { keys::LEFTALT } else { keys::RIGHTALT })]
    }

    fn promote_pending_ctrls(&mut self) -> Vec<Action> {
        let mut actions = self.promote_pending_ctrl(0);
        actions.extend(self.promote_pending_ctrl(1));
        actions
    }

    fn promote_pending_ctrl(&mut self, index: usize) -> Vec<Action> {
        if !self.ctrl[index].hidden_from_foreground() {
            return Vec::new();
        }
        self.ctrl[index].native_down_sent = true;
        vec![Action::press(if index == 0 { keys::LEFTCTRL } else { keys::RIGHTCTRL })]
    }

    fn consume_pending_alt_gestures(&mut self) {
        for state in &mut self.alt {
            if state.is_down {
                state.consumed_by_emacs = true;
            }
        }
    }

    fn consume_pending_ctrl_gestures(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        for index in 0..2 {
            if !self.ctrl[index].is_down {
                continue;
            }
            if self.ctrl[index].original_down_passed || self.ctrl[index].native_down_sent {
                // 変換の前に、前面のアプリから見える Ctrl をいったん離す。
                actions.push(Action::release(if index == 0 {
                    keys::LEFTCTRL
                } else {
                    keys::RIGHTCTRL
                }));
            }
            self.ctrl[index].original_down_passed = false;
            self.ctrl[index].native_down_sent = false;
            self.ctrl[index].consumed_by_emacs = true;
        }
        actions
    }

    /// 変換したキー列を組み立てる。Windows 版の NavigationInputPlan.Create に対応する。
    fn navigation_actions(&self, binding: &Binding) -> Vec<Action> {
        let mut native_modifiers = Vec::new();
        for (index, code) in [(0, keys::LEFTCTRL), (1, keys::RIGHTCTRL)] {
            if self.down.contains(&code) && !self.ctrl[index].hidden_from_foreground() {
                native_modifiers.push(code);
            }
        }
        for code in [keys::LEFTSHIFT, keys::RIGHTSHIFT] {
            if self.down.contains(&code) {
                native_modifiers.push(code);
            }
        }
        for (index, code) in [(0, keys::LEFTALT), (1, keys::RIGHTALT)] {
            let state = &self.alt[index];
            if state.is_down && (state.original_down_passed || state.native_down_sent) {
                native_modifiers.push(code);
            }
        }

        let mut actions = Vec::new();
        // 押しっぱなしの Ctrl / Alt / Shift が変換後の操作を汚さないよう、いったん離す。
        for code in &native_modifiers {
            actions.push(Action::release(*code));
        }
        if binding.target_control {
            actions.push(Action::press(keys::LEFTCTRL));
        }
        if binding.action == InputAction::DeleteToLineEnd {
            // 行末では Shift+End が何も選択しないので、Delete が次の行と結合する。
            actions.push(Action::press(keys::LEFTSHIFT));
            actions.push(Action::press(keys::END));
            actions.push(Action::release(keys::END));
            actions.push(Action::release(keys::LEFTSHIFT));
        }
        actions.push(Action::press(binding.target));
        actions.push(Action::release(binding.target));
        if binding.target_control {
            actions.push(Action::release(keys::LEFTCTRL));
        }
        // 離した順のまま押し直す (Windows 版の NavigationInputPlan と同じ並び)。
        for code in &native_modifiers {
            actions.push(Action::press(*code));
        }
        actions
    }

    fn navigation_modifiers(&self) -> Modifiers {
        Modifiers {
            control: self.down.contains(&keys::LEFTCTRL) || self.down.contains(&keys::RIGHTCTRL),
            alt: self.down.contains(&keys::LEFTALT) || self.down.contains(&keys::RIGHTALT),
            shift: self.shift_down(),
            meta: self.down.contains(&keys::LEFTMETA) || self.down.contains(&keys::RIGHTMETA),
        }
    }

    fn side_filter(&self) -> SideFilter {
        SideFilter {
            control_down: Sides {
                left: self.down.contains(&keys::LEFTCTRL),
                right: self.down.contains(&keys::RIGHTCTRL),
            },
            alt_down: self.alt_sides(),
            control_selection: self.config.emacs.control_side,
            alt_selection: self.config.emacs.alt_side,
        }
    }

    fn alt_sides(&self) -> Sides {
        Sides {
            left: self.down.contains(&keys::LEFTALT),
            right: self.down.contains(&keys::RIGHTALT),
        }
    }

    fn shift_down(&self) -> bool {
        self.down.contains(&keys::LEFTSHIFT) || self.down.contains(&keys::RIGHTSHIFT)
    }

    fn any_non_alt_key_down(&self) -> bool {
        self.down.iter().any(|code| !keys::is_alt(*code)) || self.emacs.gesture_active()
    }

    fn any_other_input_down_for_ctrl(&self) -> bool {
        self.down.iter().any(|code| !keys::is_ctrl(*code)) || self.emacs.gesture_active()
    }

    fn can_defer_ctrl(&self, side: CtrlSide) -> bool {
        let down = match side {
            CtrlSide::Left => Sides { left: true, right: false },
            CtrlSide::Right => Sides { left: false, right: true },
        };
        self.config.emacs_enabled()
            && self.emacs.active
            && self.config.emacs.control_side.allows(down)
    }

    fn can_defer_alt_shift(&self) -> bool {
        self.config.emacs_enabled()
            && self.emacs.active
            && self.config.should_defer_alt_shift()
            && self.config.emacs.alt_side.allows(self.alt_sides())
            && !self.down.contains(&keys::LEFTCTRL)
            && !self.down.contains(&keys::RIGHTCTRL)
            && !self.down.contains(&keys::LEFTMETA)
            && !self.down.contains(&keys::RIGHTMETA)
    }

    fn mark_deferred_shift_chord(&mut self) {
        for state in &mut self.alt {
            if state.hidden_from_foreground() {
                state.deferred_shift_chord = true;
            }
        }
    }
}
