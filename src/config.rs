//! 設定の読み書き。Windows 版の AppSettings.cs / SettingsStore.cs に対応する。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::keys::{Shortcut, SideSelection};

pub const DEFAULT_LONG_PRESS_MS: u64 = 600;
pub const MIN_LONG_PRESS_MS: u64 = 200;
pub const MAX_LONG_PRESS_MS: u64 = 3000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InitialMode {
    #[default]
    Normal,
    Emacs,
}

/// IME のオン・オフをどの手段で行うか。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImeBackend {
    /// fcitx5 の D-Bus (org.fcitx.Fcitx.Controller1) の Activate / Deactivate を呼ぶ。
    #[default]
    Fcitx5,
    /// ibus の入力ソースを切り替える (`ibus engine <名前>`)。GNOME の
    /// 「入力ソースの切り替え」(Super+Space) と同じ操作にあたる。
    Ibus,
    /// 何もしない。Alt 単押しの検出だけ試したいときに使う。
    None,
}

/// IME の切り替え方法。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ImeSettings {
    pub backend: ImeBackend,
    /// ibus バックエンドで「IME オン」にするときの入力ソース名。
    pub ibus_engine_on: String,
    /// ibus バックエンドで「IME オフ」にするときの入力ソース名。
    pub ibus_engine_off: String,
}

impl Default for ImeSettings {
    fn default() -> Self {
        ImeSettings {
            backend: ImeBackend::default(),
            ibus_engine_on: "mozc-jp".to_owned(),
            ibus_engine_off: "xkb:us::eng".to_owned(),
        }
    }
}

/// ウィンドウ移動でリサイズに使う修飾キー。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResizeModifier {
    #[default]
    Shift,
    Ctrl,
    Alt,
    /// リサイズを使わない (常に移動)。
    None,
}

/// ウィンドウ移動のきっかけにするマウスボタン。
/// 左ボタンは操作不能になるため選べない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowMoveButton {
    #[default]
    Middle,
    Right,
    Side,
    Extra,
    Forward,
    Back,
    Task,
    /// BTN_0。一部のマウスが追加ボタンに使う。
    Btn0,
}

impl WindowMoveButton {
    pub fn code(self) -> u16 {
        use evdev::KeyCode;
        match self {
            WindowMoveButton::Middle => KeyCode::BTN_MIDDLE.code(),
            WindowMoveButton::Right => KeyCode::BTN_RIGHT.code(),
            WindowMoveButton::Side => KeyCode::BTN_SIDE.code(),
            WindowMoveButton::Extra => KeyCode::BTN_EXTRA.code(),
            WindowMoveButton::Forward => KeyCode::BTN_FORWARD.code(),
            WindowMoveButton::Back => KeyCode::BTN_BACK.code(),
            WindowMoveButton::Task => KeyCode::BTN_TASK.code(),
            WindowMoveButton::Btn0 => KeyCode::BTN_0.code(),
        }
    }
}

/// マウス 1 台分のボタン割り当て。マウスごとにボタン構成が違うので、
/// デバイス名で選んで既定の割り当てを上書きする。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct DeviceRule {
    /// デバイス名の一部。大文字小文字は区別しない (例: "ELECOM")。
    pub name: String,
    pub button: Option<WindowMoveButton>,
    /// 0 以外にすると `button` より優先する。
    pub button_code: Option<u16>,
}

impl DeviceRule {
    /// このデバイスで見張るボタンのコード。
    fn code(&self) -> u16 {
        self.button_code
            .filter(|code| *code != 0)
            .unwrap_or_else(|| self.button.unwrap_or_default().code())
    }
}

/// ウィンドウマネージャーが「修飾キー+ドラッグ」に使う修飾キー。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WmModifier {
    /// GNOME の設定 (org.gnome.desktop.wm.preferences mouse-button-modifier) から読む。
    #[default]
    Auto,
    Alt,
    Super,
}

/// マウスのミドルドラッグでウィンドウを移動する機能。
///
/// Wayland には他のアプリのウィンドウを動かす API が無いので、ウィンドウマネージャーが
/// 持っている「修飾キー+ドラッグ」へ翻訳する。GNOME では修飾キー+左ドラッグが移動、
/// +中ドラッグがリサイズにあたる。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct WindowMoveConfig {
    /// ON にするとマウスを占有する。切り替えには常駐の再起動が必要。
    pub enabled: bool,
    /// きっかけにするマウスボタン。
    pub button: WindowMoveButton,
    /// 0 以外にすると、`button` より優先してこの生のコードを使う。
    /// `alt-henkan --watch-input` で調べた番号をそのまま入れられる。
    pub button_code: u16,
    /// ドラッグと判定する移動量 (ピクセル)。これを超えるまではミドルクリックを保留する。
    pub drag_threshold: i32,
    pub resize_modifier: ResizeModifier,
    pub wm_modifier: WmModifier,
    /// 修飾キーを押してからボタンを押すまでの待ち時間 (ミリ秒)。
    pub modifier_delay_ms: u64,
    /// マウスごとの上書き。前から順に、名前が一致した最初のものを使う。
    pub devices: Vec<DeviceRule>,
}

impl Default for WindowMoveConfig {
    fn default() -> Self {
        WindowMoveConfig {
            enabled: false,
            button: WindowMoveButton::default(),
            button_code: 0,
            drag_threshold: 4,
            resize_modifier: ResizeModifier::Shift,
            wm_modifier: WmModifier::Auto,
            modifier_delay_ms: 30,
            devices: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    /// OFF にすると Alt 変換と Emacs 機能の両方を停止する。
    pub enabled: bool,
    /// 左 Alt を長押しして離したとき、本来の Alt を送る。
    pub left_alt_long_press: bool,
    /// 右 Alt を長押しして離したとき、本来の Alt を送る。
    pub right_alt_long_press: bool,
    /// 長押しと判定するまでの時間 (ミリ秒)。
    pub long_press_ms: u64,
    /// モード切り替え時にデスクトップ通知を出す。
    pub show_mode_notification: bool,
    pub ime: ImeSettings,
    pub emacs: EmacsConfig,
    pub window_move: WindowMoveConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct EmacsConfig {
    /// CapsLock による Emacs モード切り替えを有効にする。
    pub enabled: bool,
    pub initial_mode: InitialMode,
    pub control_side: SideSelection,
    pub alt_side: SideSelection,
    pub shortcuts: Shortcuts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Shortcuts {
    pub control_b: bool,
    pub control_f: bool,
    pub control_p: bool,
    pub control_n: bool,
    pub control_a: bool,
    pub control_e: bool,
    pub alt_b: bool,
    pub alt_f: bool,
    pub alt_less: bool,
    pub alt_greater: bool,
    pub control_d: bool,
    pub alt_d: bool,
    pub control_k: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            left_alt_long_press: true,
            right_alt_long_press: true,
            long_press_ms: DEFAULT_LONG_PRESS_MS,
            show_mode_notification: true,
            ime: ImeSettings::default(),
            emacs: EmacsConfig::default(),
            window_move: WindowMoveConfig::default(),
        }
    }
}

impl Default for EmacsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            initial_mode: InitialMode::Normal,
            control_side: SideSelection::Both,
            alt_side: SideSelection::Both,
            shortcuts: Shortcuts::default(),
        }
    }
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            control_b: true,
            control_f: true,
            control_p: true,
            control_n: true,
            control_a: true,
            control_e: true,
            alt_b: true,
            alt_f: true,
            alt_less: true,
            alt_greater: true,
            control_d: true,
            alt_d: true,
            control_k: true,
        }
    }
}

impl Shortcuts {
    pub fn is_enabled(&self, shortcut: Shortcut) -> bool {
        match shortcut {
            Shortcut::ControlB => self.control_b,
            Shortcut::ControlF => self.control_f,
            Shortcut::ControlP => self.control_p,
            Shortcut::ControlN => self.control_n,
            Shortcut::ControlA => self.control_a,
            Shortcut::ControlE => self.control_e,
            Shortcut::AltB => self.alt_b,
            Shortcut::AltF => self.alt_f,
            Shortcut::AltLess => self.alt_less,
            Shortcut::AltGreater => self.alt_greater,
            Shortcut::ControlD => self.control_d,
            Shortcut::AltD => self.alt_d,
            Shortcut::ControlK => self.control_k,
        }
    }
}

impl Config {
    pub fn normalize(mut self) -> Self {
        self.long_press_ms = self.long_press_ms.clamp(MIN_LONG_PRESS_MS, MAX_LONG_PRESS_MS);
        self.window_move.drag_threshold = self.window_move.drag_threshold.clamp(1, 64);
        self.window_move.modifier_delay_ms = self.window_move.modifier_delay_ms.min(200);
        self
    }

    /// 起動直後に Emacs モードで始めるか。
    pub fn initial_emacs_active(&self) -> bool {
        self.enabled && self.emacs.enabled && self.emacs.initial_mode == InitialMode::Emacs
    }

    pub fn emacs_enabled(&self) -> bool {
        self.enabled && self.emacs.enabled
    }

    /// Alt+< / Alt+> のために Shift 付きの Alt を保留してよいか。
    pub fn should_defer_alt_shift(&self) -> bool {
        self.emacs.shortcuts.alt_less || self.emacs.shortcuts.alt_greater
    }

    pub fn long_press_enabled(&self, side: crate::engine::AltSide) -> bool {
        match side {
            crate::engine::AltSide::Left => self.left_alt_long_press,
            crate::engine::AltSide::Right => self.right_alt_long_press,
        }
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                let config: Config = toml::from_str(&text)?;
                // 知らないキーは serde が黙って捨てるので、綴り間違いや古い書式に気づけるよう伝える。
                for key in unknown_keys(&text) {
                    crate::info!("設定の {key} は使われていません ({})", path.display());
                }
                Ok(config.normalize())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Config::default().normalize())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// 設定ファイルへ書き戻す。既にある場合はコメントと並びを保ったまま値だけ差し替える。
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        std::fs::write(path, self.to_toml_preserving(&existing))?;
        Ok(())
    }

    /// 元の TOML のコメントと並びを保ったまま、値だけ今の設定に合わせた文字列を作る。
    /// 元の内容が空か壊れているときは既定の書式で書き出す。
    pub fn to_toml_preserving(&self, existing: &str) -> String {
        let Ok(mut document) = existing.parse::<toml_edit::DocumentMut>() else {
            return self.to_toml();
        };
        if document.as_table().is_empty() {
            return self.to_toml();
        }
        // toml_edit のシリアライザは入れ子をインラインテーブルで書き出すので、
        // [emacs.shortcuts] のような見出しを保てる to_toml() を経由する。
        let Ok(updated) = self.to_toml().parse::<toml_edit::DocumentMut>() else {
            return self.to_toml();
        };
        merge_values(document.as_table_mut(), updated.as_table());
        document.to_string()
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("設定のシリアライズに失敗しました")
    }
}

/// 既定の設定ファイルの場所 (`$XDG_CONFIG_HOME/alt-henkan/config.toml`)。
pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("alt-henkan").join("config.toml")
}

/// 設定ファイルの中で、このバージョンが知らないキーの一覧 (`ime.backend` のような形式)。
pub fn unknown_keys(text: &str) -> Vec<String> {
    const ROOT: &[&str] = &[
        "enabled",
        "left-alt-long-press",
        "right-alt-long-press",
        "long-press-ms",
        "show-mode-notification",
        "ime",
        "emacs",
        "window-move",
    ];
    const IME: &[&str] = &["backend", "ibus-engine-on", "ibus-engine-off"];
    const EMACS: &[&str] =
        &["enabled", "initial-mode", "control-side", "alt-side", "shortcuts"];
    const WINDOW_MOVE: &[&str] = &[
        "enabled",
        "button",
        "button-code",
        "drag-threshold",
        "resize-modifier",
        "wm-modifier",
        "modifier-delay-ms",
        "devices",
    ];
    const DEVICE_RULE: &[&str] = &["name", "button", "button-code"];
    const SHORTCUTS: &[&str] = &[
        "control-b",
        "control-f",
        "control-p",
        "control-n",
        "control-a",
        "control-e",
        "alt-b",
        "alt-f",
        "alt-less",
        "alt-greater",
        "control-d",
        "alt-d",
        "control-k",
    ];

    let Ok(table) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let mut unknown = Vec::new();
    collect_unknown(&table, ROOT, "", &mut unknown);
    if let Some(toml::Value::Table(ime)) = table.get("ime") {
        collect_unknown(ime, IME, "ime.", &mut unknown);
    }
    if let Some(toml::Value::Table(window_move)) = table.get("window-move") {
        collect_unknown(window_move, WINDOW_MOVE, "window-move.", &mut unknown);
        if let Some(toml::Value::Array(devices)) = window_move.get("devices") {
            for rule in devices {
                if let toml::Value::Table(rule) = rule {
                    collect_unknown(rule, DEVICE_RULE, "window-move.devices.", &mut unknown);
                }
            }
        }
    }
    if let Some(toml::Value::Table(emacs)) = table.get("emacs") {
        collect_unknown(emacs, EMACS, "emacs.", &mut unknown);
        if let Some(toml::Value::Table(shortcuts)) = emacs.get("shortcuts") {
            collect_unknown(shortcuts, SHORTCUTS, "emacs.shortcuts.", &mut unknown);
        }
    }
    unknown
}

fn collect_unknown(
    table: &toml::Table,
    known: &[&str],
    prefix: &str,
    unknown: &mut Vec<String>,
) {
    for key in table.keys() {
        if !known.contains(&key.as_str()) {
            unknown.push(format!("{prefix}{key}"));
        }
    }
}

impl WindowMoveConfig {
    /// どのマウスにも使う既定のボタンのコード。
    pub fn trigger_code(&self) -> u16 {
        let code = if self.button_code != 0 { self.button_code } else { self.button.code() };
        guard_trigger(code)
    }

    /// このデバイスで見張るボタンのコード。名前が一致する上書きがあればそれを使う。
    pub fn trigger_code_for(&self, device_name: &str) -> u16 {
        let lowered = device_name.to_lowercase();
        for rule in &self.devices {
            if !rule.name.is_empty() && lowered.contains(&rule.name.to_lowercase()) {
                return guard_trigger(rule.code());
            }
        }
        self.trigger_code()
    }

    /// このデバイス向けの上書きがあれば返す。
    pub fn rule_for(&self, device_name: &str) -> Option<&DeviceRule> {
        let lowered = device_name.to_lowercase();
        self.devices
            .iter()
            .find(|rule| !rule.name.is_empty() && lowered.contains(&rule.name.to_lowercase()))
    }
}

impl WindowMoveButton {
    /// 全種類。設定画面の選択肢に使う。
    pub const ALL: [WindowMoveButton; 8] = [
        WindowMoveButton::Middle,
        WindowMoveButton::Right,
        WindowMoveButton::Side,
        WindowMoveButton::Extra,
        WindowMoveButton::Forward,
        WindowMoveButton::Back,
        WindowMoveButton::Task,
        WindowMoveButton::Btn0,
    ];

    /// コードから名前を引く。名前が無いコードは None。
    pub fn from_code(code: u16) -> Option<WindowMoveButton> {
        WindowMoveButton::ALL.into_iter().find(|button| button.code() == code)
    }
}

impl WindowMoveConfig {
    /// このデバイスの割り当てを設定する。`None` にすると上書きを消して既定に戻す。
    pub fn set_device_button(&mut self, device_name: &str, code: Option<u16>) {
        self.devices.retain(|rule| rule.name != device_name);
        let Some(code) = code else {
            return;
        };
        // 名前で書けるものは名前で書き、読みやすい設定ファイルにする。
        let rule = match WindowMoveButton::from_code(code) {
            Some(button) => DeviceRule {
                name: device_name.to_owned(),
                button: Some(button),
                button_code: None,
            },
            None => DeviceRule {
                name: device_name.to_owned(),
                button: None,
                button_code: Some(code),
            },
        };
        self.devices.push(rule);
    }
}

/// 左ボタンを横取りするとクリックできなくなるので、既定のミドルボタンへ戻す。
fn guard_trigger(code: u16) -> u16 {
    if code == evdev::KeyCode::BTN_LEFT.code() {
        crate::info!("ウィンドウ移動に左ボタンは使えません。ミドルボタンを使います。");
        return WindowMoveButton::Middle.code();
    }
    code
}

/// `source` の値を `target` へ写す。キーに付いたコメントや行末のコメントは残す。
fn merge_values(target: &mut toml_edit::Table, source: &toml_edit::Table) {
    use toml_edit::Item;

    for (key, item) in source.iter() {
        match (target.get_mut(key), item) {
            (Some(Item::Table(target_table)), Item::Table(source_table)) => {
                merge_values(target_table, source_table);
            }
            (Some(Item::Value(target_value)), Item::Value(source_value)) => {
                // 値だけ差し替え、前後のコメントは元のものを保つ。
                let mut replacement = source_value.clone();
                *replacement.decor_mut() = target_value.decor().clone();
                *target_value = replacement;
            }
            // 元のファイルに無いキーは、そのまま足す。Table::insert はキーを
            // 更新しないので、既にあるキーのコメントは消えない。
            _ => {
                target.insert(key, item.clone());
            }
        }
    }
}
