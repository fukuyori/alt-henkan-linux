//! ミドルドラッグによるウィンドウ移動。Windows 版の wmv (WindowMover.cs) に対応する。
//!
//! Wayland には他のアプリのウィンドウを動かす API が無い。代わりに、ウィンドウ
//! マネージャーが元々持っている「修飾キー+ドラッグ」へ翻訳する。GNOME では
//! `org.gnome.desktop.wm.preferences mouse-button-modifier` (既定は `<Alt>`) を
//! 押しながら左ドラッグで移動、中ドラッグでリサイズになる。
//!
//! - ミドルドラッグ            → 修飾キー + 左ドラッグ (移動)
//! - Shift + ミドルドラッグ    → 修飾キー + 中ドラッグ (リサイズ)
//! - しきい値を超えずに離した  → ミドルクリックを対象アプリへ送り直す
//!
//! 最大化の解除・スナップ・最小サイズ・対象外ウィンドウの判定は、すべて
//! ウィンドウマネージャー本来の処理がそのまま働く。

use evdev::{EventType, KeyCode, RelativeAxisCode};

use crate::config::{ResizeModifier, WindowMoveConfig};
use crate::keys;

pub const BTN_LEFT: u16 = KeyCode::BTN_LEFT.code();
pub const BTN_MIDDLE: u16 = KeyCode::BTN_MIDDLE.code();
const REL_X: u16 = RelativeAxisCode::REL_X.0;
const REL_Y: u16 = RelativeAxisCode::REL_Y.0;

/// 仮想ポインターへの出力。修飾キーも同じデバイスから出すので、ボタンとの
/// 前後関係が入れ替わらない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    Event { type_: u16, code: u16, value: i32 },
    /// ここまでを送り終えてから、設定した時間だけ待つ。
    /// ウィンドウマネージャーが修飾キーを認識してからボタンを受け取るようにする。
    Settle,
}

impl Out {
    fn key(code: u16, value: i32) -> Out {
        Out::Event { type_: EventType::KEY.0, code, value }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    /// ミドルボタンを押したが、まだドラッグと判定していない。
    Pending { dx: i32, dy: i32, resize: bool },
    /// ドラッグ中。押している仮想ボタン。
    Dragging { button: u16 },
}

pub struct WindowMove {
    config: WindowMoveConfig,
    /// ウィンドウマネージャーの修飾キー (KEY_LEFTALT か KEY_LEFTMETA)。
    wm_modifier: u16,
    /// きっかけにするボタンのコード。
    trigger: u16,
    /// このインスタンスが担当するデバイス名。
    device: String,
    phase: Phase,
}

impl WindowMove {
    pub fn new(config: WindowMoveConfig, wm_modifier: u16) -> Self {
        Self::for_device(config, wm_modifier, "")
    }

    /// このデバイス向けの割り当てで作る。名前が空なら既定の割り当てを使う。
    pub fn for_device(config: WindowMoveConfig, wm_modifier: u16, device_name: &str) -> Self {
        let trigger = config.trigger_code_for(device_name);
        WindowMove { config, wm_modifier, trigger, device: device_name.to_owned(), phase: Phase::Idle }
    }

    /// 見張っているボタンのコード。
    pub fn trigger(&self) -> u16 {
        self.trigger
    }

    pub fn update_config(&mut self, config: WindowMoveConfig, wm_modifier: u16) -> Vec<Out> {
        self.trigger = config.trigger_code_for(&self.device);
        self.config = config;
        self.wm_modifier = wm_modifier;
        // 途中で無効にされたら、押したままの仮想ボタンを離す。
        if !self.config.enabled { self.cancel() } else { Vec::new() }
    }

    pub fn modifier_delay_ms(&self) -> u64 {
        self.config.modifier_delay_ms
    }

    pub fn dragging(&self) -> bool {
        matches!(self.phase, Phase::Dragging { .. })
    }

    /// 押したままの仮想ボタンと修飾キーを離して、状態を捨てる。
    pub fn cancel(&mut self) -> Vec<Out> {
        let actions = match self.phase {
            Phase::Dragging { button } => vec![
                Out::key(button, 0),
                Out::key(self.wm_modifier, 0),
            ],
            _ => Vec::new(),
        };
        self.phase = Phase::Idle;
        actions
    }

    /// 占有したマウスのイベント 1 件を処理する。`modifier_down` はリサイズ用の
    /// 修飾キーが物理的に押されているか。
    pub fn on_event(
        &mut self,
        type_: u16,
        code: u16,
        value: i32,
        modifier_down: bool,
    ) -> Vec<Out> {
        if !self.config.enabled {
            return vec![Out::Event { type_, code, value }];
        }
        if type_ == EventType::KEY.0 && code == self.trigger {
            return self.on_trigger_button(value, modifier_down);
        }
        if type_ == EventType::RELATIVE.0 && (code == REL_X || code == REL_Y) {
            return self.on_motion(code, value);
        }
        vec![Out::Event { type_, code, value }]
    }

    fn on_trigger_button(&mut self, value: i32, modifier_down: bool) -> Vec<Out> {
        match (self.phase, value) {
            // 押した: ドラッグになるか分かるまで保留する。
            (Phase::Idle, 1) => {
                let resize =
                    modifier_down && self.config.resize_modifier != ResizeModifier::None;
                self.phase = Phase::Pending { dx: 0, dy: 0, resize };
                Vec::new()
            }
            // オートリピートは捨てる。
            (Phase::Pending { .. } | Phase::Dragging { .. }, 2) => Vec::new(),
            // しきい値を超えずに離した: 本来のクリックとして送り直す。
            (Phase::Pending { .. }, 0) => {
                self.phase = Phase::Idle;
                vec![Out::key(self.trigger, 1), Out::key(self.trigger, 0)]
            }
            // ドラッグの終了。
            (Phase::Dragging { button }, 0) => {
                self.phase = Phase::Idle;
                vec![Out::key(button, 0), Out::Settle, Out::key(self.wm_modifier, 0)]
            }
            (_, value) => vec![Out::key(self.trigger, value)],
        }
    }

    fn on_motion(&mut self, code: u16, value: i32) -> Vec<Out> {
        let forward = Out::Event { type_: EventType::RELATIVE.0, code, value };
        let Phase::Pending { dx, dy, resize } = self.phase else {
            return vec![forward];
        };

        let (dx, dy) = if code == REL_X { (dx + value, dy) } else { (dx, dy + value) };
        let threshold = self.config.drag_threshold;
        if dx.abs() < threshold && dy.abs() < threshold {
            self.phase = Phase::Pending { dx, dy, resize };
            return vec![forward];
        }

        // ここからドラッグ。ウィンドウマネージャーの修飾キーを押してからボタンを押す。
        let button = if resize { BTN_MIDDLE } else { BTN_LEFT };
        self.phase = Phase::Dragging { button };
        vec![
            forward,
            Out::key(self.wm_modifier, 1),
            Out::Settle,
            Out::key(button, 1),
        ]
    }
}

/// リサイズ用の修飾キーに対応するキーコード。左右どちらでも効く。
pub fn resize_modifier_keys(modifier: ResizeModifier) -> &'static [u16] {
    match modifier {
        ResizeModifier::Shift => &[keys::LEFTSHIFT, keys::RIGHTSHIFT],
        ResizeModifier::Ctrl => &[keys::LEFTCTRL, keys::RIGHTCTRL],
        ResizeModifier::Alt => &[keys::LEFTALT, keys::RIGHTALT],
        ResizeModifier::None => &[],
    }
}

/// 仮想ポインターへ書き出す 1 フレーム分のイベント。
///
/// 元のマウスは X と Y の移動を 1 つのフレーム (SYN_REPORT 区切り) にまとめて
/// 送ってくる。1 件ずつ SYN_REPORT を付けて書き出すと 1 回の動きが 2 回に割れ、
/// カーソルの動きががたつく。元の区切りをそのまま保つためにここで貯める。
#[derive(Debug, Default)]
pub struct Frame {
    events: Vec<(u16, u16, i32)>,
}

impl Frame {
    pub fn new() -> Self {
        Frame::default()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// イベントを 1 件積む。同じキーが既に入っていると片方が消えてしまうので、
    /// その場合は先に書き出すぶんを返す。
    #[must_use]
    pub fn push(&mut self, type_: u16, code: u16, value: i32) -> Option<Vec<(u16, u16, i32)>> {
        let collides = self
            .events
            .iter()
            .any(|(queued_type, queued_code, _)| *queued_type == type_ && *queued_code == code);
        let flushed = collides.then(|| self.take());
        self.events.push((type_, code, value));
        flushed
    }

    /// 貯めたぶんを取り出して空にする。
    pub fn take(&mut self) -> Vec<(u16, u16, i32)> {
        std::mem::take(&mut self.events)
    }
}
