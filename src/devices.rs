//! evdev デバイスの検出・占有と、uinput への書き戻し。
//!
//! Wayland では低レベルのキーボードフックに相当する仕組みがないため、キーボードを
//! EVIOCGRAB で占有して入力を横取りし、通すイベントだけ仮想キーボードへ書き出す。

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::Duration;

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, Device, EventType, KeyCode, RelativeAxisCode};

use crate::message::Message;

pub const VIRTUAL_DEVICE_NAME: &str = "alt-henkan virtual keyboard";
pub const VIRTUAL_POINTER_NAME: &str = "alt-henkan virtual pointer";
/// 自分が作った仮想デバイスを掴み直さないための目印。
const OWN_DEVICE_PREFIX: &str = "alt-henkan virtual";

/// デバイスの扱い方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// 占有してキーを横取りする。
    Keyboard,
    /// 占有してボタン・移動を横取りする (ウィンドウ移動を使うとき)。
    Mouse,
    /// 占有せず、ボタンとホイールが押されたことだけ知る。
    Pointer,
}

/// 自分が作った仮想デバイスか。
fn is_own(device: &Device) -> bool {
    device.name().is_some_and(|name| name.starts_with(OWN_DEVICE_PREFIX))
}

/// 占有するキーボードか。文字キーを持ち、ポインターではないデバイスを対象にする。
pub fn is_keyboard(device: &Device) -> bool {
    if is_own(device) {
        return false;
    }
    let Some(keys) = device.supported_keys() else {
        return false;
    };
    let has_letters = keys.contains(KeyCode::KEY_A)
        && keys.contains(KeyCode::KEY_Z)
        && keys.contains(KeyCode::KEY_SPACE);
    has_letters && !is_pointer(device)
}

/// マウス・タッチパッドなどのポインター。
pub fn is_pointer(device: &Device) -> bool {
    if is_own(device) {
        return false;
    }
    let has_pointer_axis = device
        .supported_relative_axes()
        .is_some_and(|axes| axes.contains(RelativeAxisCode::REL_X));
    let has_pointer_button = device
        .supported_keys()
        .is_some_and(|keys| keys.contains(KeyCode::BTN_LEFT) || keys.contains(KeyCode::BTN_TOUCH));
    has_pointer_axis || has_pointer_button
}

/// 占有してよいマウスか。相対座標のポインターだけを対象にし、タッチパッドや
/// タブレットのような絶対座標のデバイスは触らない (多点接触の再現が難しいため)。
pub fn is_mouse(device: &Device) -> bool {
    if !is_pointer(device) {
        return false;
    }
    let relative = device
        .supported_relative_axes()
        .is_some_and(|axes| axes.contains(RelativeAxisCode::REL_X));
    let has_button =
        device.supported_keys().is_some_and(|keys| keys.contains(KeyCode::BTN_LEFT));
    let absolute = device.supported_absolute_axes().is_some_and(|axes| {
        axes.contains(evdev::AbsoluteAxisCode::ABS_MT_POSITION_X)
            || axes.contains(evdev::AbsoluteAxisCode::ABS_X)
    });
    relative && has_button && !absolute
}

/// Windows 版の IsMouseInteraction と同じ範囲。カーソル移動だけでは確定させない。
pub fn is_pointer_button(code: u16) -> bool {
    (KeyCode::BTN_LEFT.code()..=KeyCode::BTN_TASK.code()).contains(&code)
}

pub fn is_wheel(code: u16) -> bool {
    code == RelativeAxisCode::REL_WHEEL.0
        || code == RelativeAxisCode::REL_HWHEEL.0
        || code == RelativeAxisCode::REL_WHEEL_HI_RES.0
        || code == RelativeAxisCode::REL_HWHEEL_HI_RES.0
}

/// 書き戻し用の仮想キーボードを作る。標準的なキーコードをすべて有効にしておき、
/// どのキーボードを占有しても同じキーを送り返せるようにする。
pub fn create_sink() -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    // 1..=248 が通常のキー、352..=464 がメディア・システム系のキー。
    // その間の 256..=351 は BTN_* なので含めない (ポインターと誤認されるのを避ける)。
    for code in (1u16..=248).chain(352u16..=464) {
        keys.insert(KeyCode::new(code));
    }
    VirtualDevice::builder()?.name(VIRTUAL_DEVICE_NAME).with_keys(&keys)?.build()
}

/// 検出済みのデバイスを覚えておき、抜き差しに追従する。
pub struct DeviceWatcher {
    tracked: HashSet<PathBuf>,
    sender: Sender<Message>,
    /// 外れたデバイスの知らせを受ける口。巡回スレッドの中だけで使う。
    lost_sender: Sender<PathBuf>,
    lost: std::sync::mpsc::Receiver<PathBuf>,
    /// ウィンドウ移動を使うときだけマウスも占有する。
    grab_mice: bool,
    /// 占有したマウスに振る通し番号。
    next_mouse_id: u32,
}

/// 巡回の間隔。デバイスの抜き差しに追従する。
const SCAN_INTERVAL: Duration = Duration::from_secs(2);

impl DeviceWatcher {
    pub fn new(sender: Sender<Message>, grab_mice: bool) -> Self {
        let (lost_sender, lost) = channel();
        DeviceWatcher {
            tracked: HashSet::new(),
            sender,
            lost_sender,
            lost,
            grab_mice,
            next_mouse_id: 0,
        }
    }

    /// 巡回を専用スレッドへ移す。デバイスを開く処理は数ミリ秒かかることがあり、
    /// 入力を捌くループでやるとカーソルの動きが引っかかる。
    pub fn run_in_background(mut self) {
        thread::Builder::new()
            .name("alt-henkan-scan".into())
            .spawn(move || {
                let mut nodes = input_nodes();
                loop {
                    thread::sleep(SCAN_INTERVAL);
                    for path in self.lost.try_iter().collect::<Vec<_>>() {
                        crate::info!("デバイスが外れました: {}", path.display());
                        self.tracked.remove(&path);
                    }
                    // /dev/input の中身が変わったときだけ開きにいく。
                    let current = input_nodes();
                    if current != nodes {
                        nodes = current;
                        self.scan();
                    }
                }
            })
            .expect("巡回スレッドを起動できませんでした");
    }

    /// 新しく現れたデバイスを開いて読み取りスレッドを起こす。戻り値は今回追加した数。
    pub fn scan(&mut self) -> usize {
        let mut added = 0;
        for (path, device) in evdev::enumerate() {
            if self.tracked.contains(&path) {
                continue;
            }
            let role = if is_keyboard(&device) {
                Role::Keyboard
            } else if self.grab_mice && is_mouse(&device) {
                Role::Mouse
            } else if is_pointer(&device) {
                Role::Pointer
            } else {
                continue;
            };
            let name = device.name().unwrap_or("(名前なし)").to_owned();
            let mouse_id = if role == Role::Mouse {
                self.next_mouse_id += 1;
                // 本体がこのマウス用の割り当てを用意できるよう、先に知らせる。
                let _ = self
                    .sender
                    .send(Message::MouseAdded { device: self.next_mouse_id, name: name.clone() });
                self.next_mouse_id
            } else {
                0
            };
            match self.start(path.clone(), device, role, mouse_id) {
                Ok(()) => {
                    self.tracked.insert(path.clone());
                    added += 1;
                    let label = match role {
                        Role::Keyboard => "占有 (キーボード)",
                        Role::Mouse => "占有 (マウス)",
                        Role::Pointer => "監視 (ポインター)",
                    };
                    crate::info!("{label}: {name} ({})", path.display());
                }
                Err(error) => {
                    crate::diag!("{} を開けませんでした: {error}", path.display());
                }
            }
        }
        added
    }

    fn start(
        &self,
        path: PathBuf,
        mut device: Device,
        role: Role,
        mouse_id: u32,
    ) -> std::io::Result<()> {
        if role != Role::Pointer {
            device.grab()?;
        }
        let sender = self.sender.clone();
        let lost = self.lost_sender.clone();
        thread::Builder::new()
            .name(format!("alt-henkan-{}", path.display()))
            .spawn(move || read_loop(path, device, role, mouse_id, sender, lost))?;
        Ok(())
    }
}

fn read_loop(
    path: PathBuf,
    mut device: Device,
    role: Role,
    mouse_id: u32,
    sender: Sender<Message>,
    lost: Sender<PathBuf>,
) {
    loop {
        let events = match device.fetch_events() {
            Ok(events) => events,
            Err(error) => {
                crate::diag!("{} の読み取りを終了します: {error}", path.display());
                let _ = lost.send(path);
                return;
            }
        };
        for event in events {
            let message = match (role, event.event_type()) {
                (Role::Keyboard, EventType::KEY) if !is_pointer_button(event.code()) => {
                    Message::Key { code: event.code(), value: event.value() }
                }
                // 占有したマウスは、ボタンも移動もそのまま渡して書き戻す。
                // SYN_REPORT も渡して、元のフレームの区切りをそのまま保つ。
                (Role::Mouse, EventType::KEY | EventType::RELATIVE | EventType::SYNCHRONIZATION) =>
                Message::Pointer {
                    device: mouse_id,
                    type_: event.event_type().0,
                    code: event.code(),
                    value: event.value(),
                },
                (_, EventType::KEY)
                    if is_pointer_button(event.code()) && event.value() != 0 =>
                {
                    Message::PointerActivity
                }
                (_, EventType::RELATIVE) if is_wheel(event.code()) => Message::PointerActivity,
                _ => continue,
            };
            if sender.send(message).is_err() {
                return;
            }
        }
    }
}

/// どれかのキーボードで大文字固定が点灯しているか。
pub fn caps_lock_on() -> bool {
    evdev::enumerate().any(|(_, device)| {
        is_keyboard(&device)
            && device
                .get_led_state()
                .is_ok_and(|leds| leds.contains(evdev::LedCode::LED_CAPSL))
    })
}

/// ウィンドウ移動で使う仮想ポインター。修飾キーも同じデバイスから出すことで、
/// ボタンとの前後関係が入れ替わらないようにする。
pub fn create_pointer_sink() -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in KeyCode::BTN_LEFT.code()..=KeyCode::BTN_TASK.code() {
        keys.insert(KeyCode::new(code));
    }
    for code in [crate::keys::LEFTALT, crate::keys::LEFTMETA, crate::keys::LEFTSHIFT,
                 crate::keys::LEFTCTRL] {
        keys.insert(KeyCode::new(code));
    }
    let mut axes = AttributeSet::<RelativeAxisCode>::new();
    for axis in [
        RelativeAxisCode::REL_X,
        RelativeAxisCode::REL_Y,
        RelativeAxisCode::REL_WHEEL,
        RelativeAxisCode::REL_HWHEEL,
        RelativeAxisCode::REL_WHEEL_HI_RES,
        RelativeAxisCode::REL_HWHEEL_HI_RES,
    ] {
        axes.insert(axis);
    }
    VirtualDevice::builder()?
        .name(VIRTUAL_POINTER_NAME)
        .with_keys(&keys)?
        .with_relative_axes(&axes)?
        .build()
}

/// つながっているマウスと、そのマウスが持つボタンのコード。
/// 設定画面が「そのマウスに実際にあるボタン」だけを出すために使う。
pub fn connected_mice() -> Vec<(String, Vec<u16>)> {
    const CANDIDATES: [KeyCode; 8] = [
        KeyCode::BTN_MIDDLE,
        KeyCode::BTN_RIGHT,
        KeyCode::BTN_SIDE,
        KeyCode::BTN_EXTRA,
        KeyCode::BTN_FORWARD,
        KeyCode::BTN_BACK,
        KeyCode::BTN_TASK,
        KeyCode::BTN_0,
    ];
    let mut found: Vec<(String, Vec<u16>)> = Vec::new();
    for (_, device) in evdev::enumerate() {
        if !is_mouse(&device) {
            continue;
        }
        let name = device.name().unwrap_or("(名前なし)").to_owned();
        if found.iter().any(|(existing, _)| *existing == name) {
            continue;
        }
        let Some(keys) = device.supported_keys() else {
            continue;
        };
        let buttons =
            CANDIDATES.iter().filter(|key| keys.contains(**key)).map(|key| key.code()).collect();
        found.push((name, buttons));
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// /dev/input にある event ノードの一覧。全デバイスを開くより桁違いに軽いので、
/// 変化があったときだけ実際に開きにいく。
fn input_nodes() -> std::collections::BTreeSet<OsString> {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return std::collections::BTreeSet::new();
    };
    entries
        .flatten()
        .map(|entry| entry.file_name())
        .filter(|name| name.as_encoded_bytes().starts_with(b"event"))
        .collect()
}
