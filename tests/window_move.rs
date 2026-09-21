//! ミドルドラッグによるウィンドウ移動の判定。Windows 版の wmv に対応する。

use alt_henkan::config::{ResizeModifier, WindowMoveButton, WindowMoveConfig};
use alt_henkan::keys;
use alt_henkan::window_move::{BTN_LEFT, BTN_MIDDLE, Out, WindowMove};

const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;

fn event(type_: u16, code: u16, value: i32) -> Out {
    Out::Event { type_, code, value }
}

fn key(code: u16, value: i32) -> Out {
    event(EV_KEY, code, value)
}

fn motion(code: u16, value: i32) -> Out {
    event(EV_REL, code, value)
}

fn enabled() -> WindowMoveConfig {
    WindowMoveConfig { enabled: true, ..WindowMoveConfig::default() }
}

/// 既定はしきい値 4px、リサイズは Shift、修飾キーは Alt。
fn mover() -> WindowMove {
    WindowMove::new(enabled(), keys::LEFTALT)
}

/// マウスの移動を 1 イベントずつ流す。
fn drag(mover: &mut WindowMove, dx: i32, modifier: bool) -> Vec<Out> {
    mover.on_event(EV_REL, REL_X, dx, modifier)
}

#[test]
fn 無効なら素通しする() {
    let mut mover = WindowMove::new(WindowMoveConfig::default(), keys::LEFTALT);
    assert_eq!(mover.on_event(EV_KEY, BTN_MIDDLE, 1, false), vec![key(BTN_MIDDLE, 1)]);
    assert_eq!(mover.on_event(EV_KEY, BTN_MIDDLE, 0, false), vec![key(BTN_MIDDLE, 0)]);
}

#[test]
fn 動かさずに離したらミドルクリックを送り直す() {
    let mut mover = mover();
    // 押した時点では何も送らない。ドラッグになるか分からないため。
    assert_eq!(mover.on_event(EV_KEY, BTN_MIDDLE, 1, false), vec![]);
    assert_eq!(drag(&mut mover, 2, false), vec![motion(REL_X, 2)]);
    assert_eq!(
        mover.on_event(EV_KEY, BTN_MIDDLE, 0, false),
        vec![key(BTN_MIDDLE, 1), key(BTN_MIDDLE, 0)]
    );
    assert!(!mover.dragging());
}

#[test]
fn しきい値を超えたら修飾キー付きの左ドラッグになる() {
    let mut mover = mover();
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, false);
    assert_eq!(drag(&mut mover, 3, false), vec![motion(REL_X, 3)]);
    // 3 + 1 = 4 でしきい値に達する。移動を通した後で修飾キーとボタンを押す。
    assert_eq!(
        drag(&mut mover, 1, false),
        vec![
            motion(REL_X, 1),
            key(keys::LEFTALT, 1),
            Out::Settle,
            key(BTN_LEFT, 1),
        ]
    );
    assert!(mover.dragging());
    // 以降の移動はそのまま通す。
    assert_eq!(drag(&mut mover, 10, false), vec![motion(REL_X, 10)]);
    assert_eq!(
        mover.on_event(EV_KEY, BTN_MIDDLE, 0, false),
        vec![key(BTN_LEFT, 0), Out::Settle, key(keys::LEFTALT, 0)]
    );
    assert!(!mover.dragging());
}

#[test]
fn 縦方向の移動でもドラッグになる() {
    let mut mover = mover();
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, false);
    let out = mover.on_event(EV_REL, REL_Y, 5, false);
    assert_eq!(out.last(), Some(&key(BTN_LEFT, 1)));
}

#[test]
fn 修飾キーを押しながらならリサイズになる() {
    let mut mover = mover();
    // 押した瞬間の修飾キーの状態で決まる。
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, true);
    assert_eq!(
        drag(&mut mover, 10, true),
        vec![
            motion(REL_X, 10),
            key(keys::LEFTALT, 1),
            Out::Settle,
            key(BTN_MIDDLE, 1),
        ]
    );
    assert_eq!(
        mover.on_event(EV_KEY, BTN_MIDDLE, 0, true),
        vec![key(BTN_MIDDLE, 0), Out::Settle, key(keys::LEFTALT, 0)]
    );
}

#[test]
fn 途中で修飾キーを押してもリサイズにはならない() {
    let mut mover = mover();
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, false);
    let out = drag(&mut mover, 10, true);
    assert_eq!(out.last(), Some(&key(BTN_LEFT, 1)), "押した時点の状態で決めるべき");
}

#[test]
fn リサイズをoffにすると常に移動になる() {
    let config = WindowMoveConfig { resize_modifier: ResizeModifier::None, ..enabled() };
    let mut mover = WindowMove::new(config, keys::LEFTALT);
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, true);
    let out = drag(&mut mover, 10, true);
    assert_eq!(out.last(), Some(&key(BTN_LEFT, 1)));
}

#[test]
fn 修飾キーはsuperにもできる() {
    let mut mover = WindowMove::new(enabled(), keys::LEFTMETA);
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, false);
    let out = drag(&mut mover, 10, false);
    assert!(out.contains(&key(keys::LEFTMETA, 1)));
}

#[test]
fn しきい値は設定で変えられる() {
    let config = WindowMoveConfig { drag_threshold: 20, ..enabled() };
    let mut mover = WindowMove::new(config, keys::LEFTALT);
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, false);
    assert_eq!(drag(&mut mover, 19, false), vec![motion(REL_X, 19)]);
    assert!(drag(&mut mover, 1, false).contains(&key(BTN_LEFT, 1)));
}

#[test]
fn 左右のボタンやホイールはそのまま通す() {
    let mut mover = mover();
    let btn_right = evdev::KeyCode::BTN_RIGHT.code();
    let rel_wheel = evdev::RelativeAxisCode::REL_WHEEL.0;
    assert_eq!(mover.on_event(EV_KEY, BTN_LEFT, 1, false), vec![key(BTN_LEFT, 1)]);
    assert_eq!(mover.on_event(EV_KEY, btn_right, 1, false), vec![key(btn_right, 1)]);
    assert_eq!(
        mover.on_event(EV_REL, rel_wheel, -1, false),
        vec![event(EV_REL, rel_wheel, -1)]
    );
}

#[test]
fn ドラッグ中のオートリピートは捨てる() {
    let mut mover = mover();
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, false);
    assert_eq!(mover.on_event(EV_KEY, BTN_MIDDLE, 2, false), vec![]);
}

#[test]
fn 途中で無効にしたら押したままのボタンを離す() {
    let mut mover = mover();
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, false);
    drag(&mut mover, 10, false);
    assert!(mover.dragging());
    let out = mover.update_config(WindowMoveConfig::default(), keys::LEFTALT);
    assert_eq!(out, vec![key(BTN_LEFT, 0), key(keys::LEFTALT, 0)]);
    assert!(!mover.dragging());
}

#[test]
fn 終了時に押したままのボタンを離す() {
    let mut mover = mover();
    mover.on_event(EV_KEY, BTN_MIDDLE, 1, false);
    drag(&mut mover, 10, false);
    assert_eq!(mover.cancel(), vec![key(BTN_LEFT, 0), key(keys::LEFTALT, 0)]);
    // 何も握っていなければ何も送らない。
    assert_eq!(mover.cancel(), vec![]);
}

#[test]
fn 設定は範囲に丸められる() {
    use alt_henkan::config::Config;
    let config = Config {
        window_move: WindowMoveConfig {
            drag_threshold: 0,
            modifier_delay_ms: 9999,
            ..enabled()
        },
        ..Config::default()
    }
    .normalize();
    assert_eq!(config.window_move.drag_threshold, 1);
    assert_eq!(config.window_move.modifier_delay_ms, 200);
}

#[test]
fn きっかけのボタンを変えられる() {
    let btn_side = evdev::KeyCode::BTN_SIDE.code();
    let config = WindowMoveConfig { button: WindowMoveButton::Side, ..enabled() };
    let mut mover = WindowMove::new(config, keys::LEFTALT);
    assert_eq!(mover.trigger(), btn_side);

    // 中ボタンはもう横取りしないので、そのまま通る。
    assert_eq!(mover.on_event(EV_KEY, BTN_MIDDLE, 1, false), vec![key(BTN_MIDDLE, 1)]);
    assert_eq!(mover.on_event(EV_KEY, BTN_MIDDLE, 0, false), vec![key(BTN_MIDDLE, 0)]);

    // サイドボタンのドラッグがウィンドウ移動になる。
    assert_eq!(mover.on_event(EV_KEY, btn_side, 1, false), vec![]);
    assert_eq!(
        drag(&mut mover, 10, false),
        vec![motion(REL_X, 10), key(keys::LEFTALT, 1), Out::Settle, key(BTN_LEFT, 1)]
    );
    assert_eq!(
        mover.on_event(EV_KEY, btn_side, 0, false),
        vec![key(BTN_LEFT, 0), Out::Settle, key(keys::LEFTALT, 0)]
    );
}

#[test]
fn きっかけのボタンを動かさず離すとそのボタンのクリックになる() {
    let btn_side = evdev::KeyCode::BTN_SIDE.code();
    let config = WindowMoveConfig { button: WindowMoveButton::Side, ..enabled() };
    let mut mover = WindowMove::new(config, keys::LEFTALT);
    mover.on_event(EV_KEY, btn_side, 1, false);
    assert_eq!(
        mover.on_event(EV_KEY, btn_side, 0, false),
        vec![key(btn_side, 1), key(btn_side, 0)]
    );
}

#[test]
fn ボタンのコードを直接指定できる() {
    let config = WindowMoveConfig {
        button: WindowMoveButton::Middle,
        button_code: 277,
        ..enabled()
    };
    let mover = WindowMove::new(config, keys::LEFTALT);
    // 直接指定が名前より優先される。
    assert_eq!(mover.trigger(), 277);
}

#[test]
fn 左ボタンを指定したら中ボタンへ戻す() {
    let config = WindowMoveConfig { button_code: BTN_LEFT, ..enabled() };
    let mover = WindowMove::new(config, keys::LEFTALT);
    // 左ボタンを横取りすると操作不能になるので受け付けない。
    assert_eq!(mover.trigger(), BTN_MIDDLE);
}

// ---- マウスごとの割り当て ----

use alt_henkan::config::DeviceRule;

fn rule(name: &str, button: WindowMoveButton) -> DeviceRule {
    DeviceRule { name: name.to_owned(), button: Some(button), button_code: None }
}

#[test]
fn マウスごとに別のボタンを割り当てられる() {
    let config = WindowMoveConfig {
        button: WindowMoveButton::Middle,
        devices: vec![
            rule("ELECOM", WindowMoveButton::Forward),
            rule("Razer", WindowMoveButton::Side),
        ],
        ..enabled()
    };
    assert_eq!(
        config.trigger_code_for("ELECOM ELECOM TrackBall Mouse"),
        evdev::KeyCode::BTN_FORWARD.code()
    );
    assert_eq!(
        config.trigger_code_for("Razer Razer Viper Ultimate Dongle"),
        evdev::KeyCode::BTN_SIDE.code()
    );
    // 一致するものが無ければ既定を使う。
    assert_eq!(config.trigger_code_for("ARCHISS M04US Mouse"), BTN_MIDDLE);
}

#[test]
fn デバイス名の一致は大文字小文字を区別しない() {
    let config =
        WindowMoveConfig { devices: vec![rule("elecom", WindowMoveButton::Side)], ..enabled() };
    assert_eq!(
        config.trigger_code_for("ELECOM ELECOM TrackBall Mouse"),
        evdev::KeyCode::BTN_SIDE.code()
    );
}

#[test]
fn 前にある割り当てが優先される() {
    let config = WindowMoveConfig {
        devices: vec![
            rule("Mouse", WindowMoveButton::Side),
            rule("ELECOM", WindowMoveButton::Forward),
        ],
        ..enabled()
    };
    assert_eq!(
        config.trigger_code_for("ELECOM ELECOM TrackBall Mouse"),
        evdev::KeyCode::BTN_SIDE.code(),
        "先に書いたものを使うべき"
    );
}

#[test]
fn 空の名前は誰にも一致しない() {
    let config =
        WindowMoveConfig { devices: vec![rule("", WindowMoveButton::Side)], ..enabled() };
    assert_eq!(config.trigger_code_for("ELECOM TrackBall"), BTN_MIDDLE);
}

#[test]
fn デバイスごとの状態は混ざらない() {
    let config = WindowMoveConfig {
        devices: vec![rule("ELECOM", WindowMoveButton::Forward)],
        ..enabled()
    };
    let btn_forward = evdev::KeyCode::BTN_FORWARD.code();
    let mut trackball =
        WindowMove::for_device(config.clone(), keys::LEFTALT, "ELECOM TrackBall Mouse");
    let mut other = WindowMove::for_device(config, keys::LEFTALT, "ARCHISS M04US Mouse");
    assert_eq!(trackball.trigger(), btn_forward);
    assert_eq!(other.trigger(), BTN_MIDDLE);

    // トラックボールで進むボタンのドラッグを始めても、もう一方は何も握っていない。
    trackball.on_event(EV_KEY, btn_forward, 1, false);
    drag(&mut trackball, 10, false);
    assert!(trackball.dragging());
    assert!(!other.dragging());

    // もう一方で同じコードを押しても、そちらでは素通しになる。
    assert_eq!(
        other.on_event(EV_KEY, btn_forward, 1, false),
        vec![key(btn_forward, 1)]
    );
}

#[test]
fn 割り当ての追加と削除ができる() {
    let mut config = enabled();
    config.set_device_button("ELECOM TrackBall Mouse", Some(evdev::KeyCode::BTN_SIDE.code()));
    assert_eq!(config.devices.len(), 1);
    // 名前で書けるものは名前で保存する。
    assert_eq!(config.devices[0].button, Some(WindowMoveButton::Side));
    assert_eq!(config.devices[0].button_code, None);

    // 同じデバイスへの設定は置き換える。
    config.set_device_button("ELECOM TrackBall Mouse", Some(999));
    assert_eq!(config.devices.len(), 1);
    assert_eq!(config.devices[0].button_code, Some(999), "名前の無いコードはそのまま保存する");

    config.set_device_button("ELECOM TrackBall Mouse", None);
    assert!(config.devices.is_empty(), "既定に戻したら上書きを消すべき");
}

#[test]
fn 割り当てを含む設定がtomlで往復できる() {
    use alt_henkan::config::Config;
    let mut config = Config { window_move: enabled(), ..Config::default() };
    config.window_move.set_device_button("ELECOM TrackBall Mouse", Some(evdev::KeyCode::BTN_FORWARD.code()));
    config.window_move.set_device_button("Razer Viper", Some(831));
    let text = config.to_toml();
    let parsed: Config = toml::from_str(&text).expect("読み直せませんでした");
    assert_eq!(parsed, config);
    assert!(alt_henkan::config::unknown_keys(&text).is_empty(), "知らないキーが出た:\n{text}");
}

// ---- フレームの組み立て ----

use alt_henkan::window_move::Frame;

#[test]
fn xとyの移動は1フレームにまとめる() {
    // 1 件ずつ書き出すと 1 回の動きが 2 回に割れ、カーソルががたつく。
    let mut frame = Frame::new();
    assert_eq!(frame.push(EV_REL, REL_X, 3), None);
    assert_eq!(frame.push(EV_REL, REL_Y, 2), None);
    assert_eq!(frame.take(), vec![(EV_REL, REL_X, 3), (EV_REL, REL_Y, 2)]);
    assert!(frame.is_empty());
}

#[test]
fn 同じキーが重なるときは先に書き出す() {
    // 押下と離上が同じフレームに入ると片方が消えてしまう。
    let mut frame = Frame::new();
    assert_eq!(frame.push(EV_KEY, BTN_MIDDLE, 1), None);
    assert_eq!(
        frame.push(EV_KEY, BTN_MIDDLE, 0),
        Some(vec![(EV_KEY, BTN_MIDDLE, 1)]),
        "押下を先に書き出すべき"
    );
    assert_eq!(frame.take(), vec![(EV_KEY, BTN_MIDDLE, 0)]);
}

#[test]
fn 違うキーなら同じフレームに入れてよい() {
    let mut frame = Frame::new();
    assert_eq!(frame.push(EV_KEY, BTN_MIDDLE, 1), None);
    assert_eq!(frame.push(EV_KEY, BTN_LEFT, 1), None);
    assert_eq!(frame.push(EV_REL, REL_X, 5), None);
    assert_eq!(frame.take().len(), 3);
}

#[test]
fn 空のフレームは何も返さない() {
    let mut frame = Frame::new();
    assert!(frame.is_empty());
    assert!(frame.take().is_empty());
}
