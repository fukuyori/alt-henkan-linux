//! 入力変換ロジックの検証。Windows 版の AltHenkan.LogicTests に対応する。

use alt_henkan::config::{Config, InitialMode};
use alt_henkan::engine::{Action, Engine, ImeCommand};
use alt_henkan::keys::{self, SideSelection};

const DOWN: i32 = 1;
const UP: i32 = 0;
const REPEAT: i32 = 2;

fn press(code: u16) -> Action {
    Action::Key { code, value: 1 }
}

fn release(code: u16) -> Action {
    Action::Key { code, value: 0 }
}

/// Alt 変換だけを有効にした既定設定。
fn alt_only() -> Config {
    Config::default()
}

/// Emacs モードを有効にし、最初から Emacs モードで始まる設定。
fn emacs_active() -> Config {
    let mut config = Config::default();
    config.emacs.enabled = true;
    config.emacs.initial_mode = InitialMode::Emacs;
    config
}

/// 診断用の Action は入力に影響しないので、比較の前に取り除く。
fn keys_only(actions: Vec<Action>) -> Vec<Action> {
    actions.into_iter().filter(|action| !matches!(action, Action::Emacs(_))).collect()
}

struct Harness {
    engine: Engine,
    now_ms: u64,
}

impl Harness {
    fn new(config: Config) -> Self {
        Harness { engine: Engine::new(config), now_ms: 10_000 }
    }

    fn advance(&mut self, ms: u64) -> &mut Self {
        self.now_ms += ms;
        self
    }

    fn key(&mut self, code: u16, value: i32) -> Vec<Action> {
        keys_only(self.engine.on_key(code, value, self.now_ms))
    }

    fn tap(&mut self, code: u16) -> Vec<Action> {
        let mut actions = self.key(code, DOWN);
        actions.extend(self.key(code, UP));
        actions
    }

    fn pointer(&mut self) -> Vec<Action> {
        keys_only(self.engine.on_pointer_activity())
    }
}

// ---- Alt 単押し ----

#[test]
fn 左alt単押しでimeをオフにする() {
    let mut h = Harness::new(alt_only());
    assert_eq!(h.key(keys::LEFTALT, DOWN), vec![]);
    h.advance(80);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![Action::Ime(ImeCommand::Off)]);
}

#[test]
fn 右alt単押しでimeをオンにする() {
    let mut h = Harness::new(alt_only());
    h.key(keys::RIGHTALT, DOWN);
    h.advance(80);
    assert_eq!(h.key(keys::RIGHTALT, UP), vec![Action::Ime(ImeCommand::On)]);
}

#[test]
fn 長押しして離すと本来のaltを送る() {
    let mut h = Harness::new(alt_only());
    h.key(keys::LEFTALT, DOWN);
    h.advance(600);
    assert_eq!(
        h.key(keys::LEFTALT, UP),
        vec![press(keys::LEFTALT), release(keys::LEFTALT)]
    );
}

#[test]
fn しきい値の直前は変換になる() {
    let mut h = Harness::new(alt_only());
    h.key(keys::LEFTALT, DOWN);
    h.advance(599);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![Action::Ime(ImeCommand::Off)]);
}

#[test]
fn 長押しを無効にした側は長く押しても変換する() {
    let mut config = alt_only();
    config.left_alt_long_press = false;
    let mut h = Harness::new(config);
    h.key(keys::LEFTALT, DOWN);
    h.advance(5_000);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![Action::Ime(ImeCommand::Off)]);
}

#[test]
fn 長押しの判定時間は設定で変えられる() {
    let mut config = alt_only();
    config.long_press_ms = 250;
    let mut h = Harness::new(config);
    h.key(keys::RIGHTALT, DOWN);
    h.advance(250);
    assert_eq!(
        h.key(keys::RIGHTALT, UP),
        vec![press(keys::RIGHTALT), release(keys::RIGHTALT)]
    );
}

#[test]
fn 判定時間は設定の範囲に丸められる() {
    let mut config = alt_only();
    config.long_press_ms = 10;
    assert_eq!(config.clone().normalize().long_press_ms, 200);
    config.long_press_ms = 99_999;
    assert_eq!(config.normalize().long_press_ms, 3_000);
}

#[test]
fn altを押したまま他のキーを押すと通常のaltになる() {
    let mut h = Harness::new(alt_only());
    assert_eq!(h.key(keys::LEFTALT, DOWN), vec![]);
    // 保留していた Alt を先に送ってから、押されたキーを送る。
    assert_eq!(
        h.key(keys::KEY_F, DOWN),
        vec![press(keys::LEFTALT), press(keys::KEY_F)]
    );
    assert_eq!(h.key(keys::KEY_F, UP), vec![release(keys::KEY_F)]);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![release(keys::LEFTALT)]);
}

#[test]
fn 他のキーを押したままaltを押すとその場で通す() {
    let mut h = Harness::new(alt_only());
    assert_eq!(h.key(keys::KEY_F, DOWN), vec![press(keys::KEY_F)]);
    assert_eq!(h.key(keys::LEFTALT, DOWN), vec![press(keys::LEFTALT)]);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![release(keys::LEFTALT)]);
}

#[test]
fn altを押したままマウスを使うと通常のaltになる() {
    let mut h = Harness::new(alt_only());
    h.key(keys::LEFTALT, DOWN);
    assert_eq!(h.pointer(), vec![press(keys::LEFTALT)]);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![release(keys::LEFTALT)]);
}

#[test]
fn 左右のaltを同時に押すと両方とも通常のaltになる() {
    let mut h = Harness::new(alt_only());
    assert_eq!(h.key(keys::LEFTALT, DOWN), vec![]);
    assert_eq!(
        h.key(keys::RIGHTALT, DOWN),
        vec![press(keys::LEFTALT), press(keys::RIGHTALT)]
    );
    assert_eq!(h.key(keys::LEFTALT, UP), vec![release(keys::LEFTALT)]);
    assert_eq!(h.key(keys::RIGHTALT, UP), vec![release(keys::RIGHTALT)]);
}

#[test]
fn 保留中のaltのオートリピートは送らない() {
    let mut h = Harness::new(alt_only());
    h.key(keys::LEFTALT, DOWN);
    assert_eq!(h.key(keys::LEFTALT, REPEAT), vec![]);
    h.advance(80);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![Action::Ime(ImeCommand::Off)]);
}

#[test]
fn 無効にすると素通しする() {
    let mut config = alt_only();
    config.enabled = false;
    let mut h = Harness::new(config);
    assert_eq!(h.key(keys::LEFTALT, DOWN), vec![press(keys::LEFTALT)]);
    h.advance(80);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![release(keys::LEFTALT)]);
}

// ---- Emacs モードの切り替え ----

#[test]
fn capslockでモードを切り替える() {
    let mut config = Config::default();
    config.emacs.enabled = true;
    let mut h = Harness::new(config);
    assert!(!h.engine.emacs_active());
    assert_eq!(
        h.key(keys::CAPSLOCK, DOWN),
        vec![Action::ModeChanged { emacs_active: true }]
    );
    assert_eq!(h.key(keys::CAPSLOCK, UP), vec![]);
    assert!(h.engine.emacs_active());
    assert_eq!(
        h.tap(keys::CAPSLOCK),
        vec![Action::ModeChanged { emacs_active: false }]
    );
    assert!(!h.engine.emacs_active());
}

#[test]
fn 機能がoffならcapslockは本来の大文字固定として通す() {
    let mut h = Harness::new(alt_only());
    assert_eq!(
        h.tap(keys::CAPSLOCK),
        vec![press(keys::CAPSLOCK), release(keys::CAPSLOCK)]
    );
    assert!(!h.engine.emacs_active());
}

#[test]
fn capslockのオートリピートではモードが揺れない() {
    let mut config = Config::default();
    config.emacs.enabled = true;
    let mut h = Harness::new(config);
    h.key(keys::CAPSLOCK, DOWN);
    assert_eq!(h.key(keys::CAPSLOCK, REPEAT), vec![]);
    assert_eq!(h.key(keys::CAPSLOCK, REPEAT), vec![]);
    h.key(keys::CAPSLOCK, UP);
    assert!(h.engine.emacs_active());
}

#[test]
fn 初期モードの設定に従って起動する() {
    assert!(Engine::new(emacs_active()).emacs_active());
    let mut config = emacs_active();
    config.enabled = false;
    assert!(!Engine::new(config).emacs_active());
}

#[test]
fn 機能をoffにすると通常モードへ戻る() {
    let mut h = Harness::new(emacs_active());
    let mut config = emacs_active();
    config.emacs.enabled = false;
    assert_eq!(
        h.engine.update_config(config),
        vec![Action::ModeChanged { emacs_active: false }]
    );
    assert!(!h.engine.emacs_active());
}

// ---- Emacs 変換 ----

#[test]
fn ctrl_bを左矢印に変換しctrlを漏らさない() {
    let mut h = Harness::new(emacs_active());
    assert_eq!(h.key(keys::LEFTCTRL, DOWN), vec![]);
    assert_eq!(
        h.key(keys::KEY_B, DOWN),
        vec![press(keys::LEFT), release(keys::LEFT)]
    );
    assert_eq!(h.key(keys::KEY_B, UP), vec![]);
    assert_eq!(h.key(keys::LEFTCTRL, UP), vec![]);
}

#[test]
fn すべての移動系ショートカットが対応するキーになる() {
    let cases: [(u16, u16); 6] = [
        (keys::KEY_B, keys::LEFT),
        (keys::KEY_F, keys::RIGHT),
        (keys::KEY_P, keys::UP),
        (keys::KEY_N, keys::DOWN),
        (keys::KEY_A, keys::HOME),
        (keys::KEY_E, keys::END),
    ];
    for (source, target) in cases {
        let mut h = Harness::new(emacs_active());
        h.key(keys::LEFTCTRL, DOWN);
        assert_eq!(
            h.key(source, DOWN),
            vec![press(target), release(target)],
            "Ctrl+{source} の変換先が違います"
        );
    }
}

#[test]
fn ctrl_dはdeleteになる() {
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTCTRL, DOWN);
    assert_eq!(
        h.key(keys::KEY_D, DOWN),
        vec![press(keys::DELETE), release(keys::DELETE)]
    );
}

#[test]
fn ctrl_kは行末まで選択してから削除する() {
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTCTRL, DOWN);
    assert_eq!(
        h.key(keys::KEY_K, DOWN),
        vec![
            press(keys::LEFTSHIFT),
            press(keys::END),
            release(keys::END),
            release(keys::LEFTSHIFT),
            press(keys::DELETE),
            release(keys::DELETE),
        ]
    );
}

#[test]
fn alt_bはctrl付きの左矢印になる() {
    let mut h = Harness::new(emacs_active());
    assert_eq!(h.key(keys::LEFTALT, DOWN), vec![]);
    assert_eq!(
        h.key(keys::KEY_B, DOWN),
        vec![
            press(keys::LEFTCTRL),
            press(keys::LEFT),
            release(keys::LEFT),
            release(keys::LEFTCTRL),
        ]
    );
    // Emacs 変換に使った Alt は、離しても IME を切り替えない。
    h.key(keys::KEY_B, UP);
    h.advance(80);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![]);
}

#[test]
fn alt_dはctrl付きのdeleteになり切り取りを行わない() {
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTALT, DOWN);
    let actions = h.key(keys::KEY_D, DOWN);
    assert_eq!(
        actions,
        vec![
            press(keys::LEFTCTRL),
            press(keys::DELETE),
            release(keys::DELETE),
            release(keys::LEFTCTRL),
        ]
    );
}

#[test]
fn alt_小なりはshiftを外してctrl_homeを送る() {
    let mut h = Harness::new(emacs_active());
    assert_eq!(h.key(keys::LEFTALT, DOWN), vec![]);
    // Alt+< は Shift を伴うので、Shift だけでは Alt を確定させない。
    assert_eq!(h.key(keys::LEFTSHIFT, DOWN), vec![press(keys::LEFTSHIFT)]);
    assert_eq!(
        h.key(keys::COMMA, DOWN),
        vec![
            release(keys::LEFTSHIFT),
            press(keys::LEFTCTRL),
            press(keys::HOME),
            release(keys::HOME),
            release(keys::LEFTCTRL),
            press(keys::LEFTSHIFT),
        ]
    );
}

#[test]
fn alt_大なりはctrl_endを送る() {
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTALT, DOWN);
    h.key(keys::LEFTSHIFT, DOWN);
    assert_eq!(
        h.key(keys::DOT, DOWN),
        vec![
            release(keys::LEFTSHIFT),
            press(keys::LEFTCTRL),
            press(keys::END),
            release(keys::END),
            release(keys::LEFTCTRL),
            press(keys::LEFTSHIFT),
        ]
    );
}

#[test]
fn shiftなしのalt_カンマは変換しない() {
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTALT, DOWN);
    assert_eq!(
        h.key(keys::COMMA, DOWN),
        vec![press(keys::LEFTALT), press(keys::COMMA)]
    );
}

#[test]
fn shiftを押したままaltを離すと本来のaltを送る() {
    // Alt+< / Alt+> のために隠していた Alt を、角括弧が来なかったので打鍵として送る。
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTALT, DOWN);
    assert_eq!(h.key(keys::LEFTSHIFT, DOWN), vec![press(keys::LEFTSHIFT)]);
    h.advance(80);
    assert_eq!(
        h.key(keys::LEFTALT, UP),
        vec![press(keys::LEFTALT), release(keys::LEFTALT)]
    );
    assert_eq!(h.key(keys::LEFTSHIFT, UP), vec![release(keys::LEFTSHIFT)]);
}

#[test]
fn 先にshiftを離すと保留していたaltを確定させる() {
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTALT, DOWN);
    h.key(keys::LEFTSHIFT, DOWN);
    // Shift を離した時点で角括弧の可能性が消えるので、Alt を本来の修飾キーにする。
    assert_eq!(
        h.key(keys::LEFTSHIFT, UP),
        vec![press(keys::LEFTALT), release(keys::LEFTSHIFT)]
    );
    h.advance(80);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![release(keys::LEFTALT)]);
}

#[test]
fn 角括弧が無効なら通常のalt_shiftとして扱う() {
    let mut config = emacs_active();
    config.emacs.shortcuts.alt_less = false;
    config.emacs.shortcuts.alt_greater = false;
    let mut h = Harness::new(config);
    h.key(keys::LEFTALT, DOWN);
    // 保留する理由が無いので、Shift の押下と同時に Alt を確定させる。
    assert_eq!(
        h.key(keys::LEFTSHIFT, DOWN),
        vec![press(keys::LEFTALT), press(keys::LEFTSHIFT)]
    );
}

#[test]
fn 変換対象外のctrl組み合わせはそのまま通す() {
    let mut h = Harness::new(emacs_active());
    assert_eq!(h.key(keys::LEFTCTRL, DOWN), vec![]);
    let v = evdev::KeyCode::KEY_V.code();
    // 保留していた Ctrl を先に送ってから V を通す。
    assert_eq!(h.key(v, DOWN), vec![press(keys::LEFTCTRL), press(v)]);
    assert_eq!(h.key(v, UP), vec![release(v)]);
    assert_eq!(h.key(keys::LEFTCTRL, UP), vec![release(keys::LEFTCTRL)]);
}

#[test]
fn ctrl単独の打鍵は本来のctrlとして送る() {
    let mut h = Harness::new(emacs_active());
    assert_eq!(h.key(keys::LEFTCTRL, DOWN), vec![]);
    assert_eq!(
        h.key(keys::LEFTCTRL, UP),
        vec![press(keys::LEFTCTRL), release(keys::LEFTCTRL)]
    );
}

#[test]
fn ctrlを押したままマウスを使うと通常のctrlになる() {
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTCTRL, DOWN);
    assert_eq!(h.pointer(), vec![press(keys::LEFTCTRL)]);
    assert_eq!(h.key(keys::LEFTCTRL, UP), vec![release(keys::LEFTCTRL)]);
}

#[test]
fn offにしたショートカットは本来の操作として通す() {
    let mut config = emacs_active();
    config.emacs.shortcuts.control_a = false;
    let mut h = Harness::new(config);
    h.key(keys::LEFTCTRL, DOWN);
    assert_eq!(
        h.key(keys::KEY_A, DOWN),
        vec![press(keys::LEFTCTRL), press(keys::KEY_A)]
    );
}

#[test]
fn 通常モードでは変換しない() {
    let mut config = Config::default();
    config.emacs.enabled = true;
    let mut h = Harness::new(config);
    // 通常モードでは Ctrl を保留しないので、押した時点でそのまま通る。
    assert_eq!(h.key(keys::LEFTCTRL, DOWN), vec![press(keys::LEFTCTRL)]);
    assert_eq!(h.key(keys::KEY_B, DOWN), vec![press(keys::KEY_B)]);
}

#[test]
fn ctrlの対象を左にすると右ctrlは変換しない() {
    let mut config = emacs_active();
    config.emacs.control_side = SideSelection::Left;
    let mut h = Harness::new(config);
    assert_eq!(h.key(keys::RIGHTCTRL, DOWN), vec![press(keys::RIGHTCTRL)]);
    assert_eq!(h.key(keys::KEY_B, DOWN), vec![press(keys::KEY_B)]);

    let mut config = emacs_active();
    config.emacs.control_side = SideSelection::Left;
    let mut h = Harness::new(config);
    h.key(keys::LEFTCTRL, DOWN);
    assert_eq!(
        h.key(keys::KEY_B, DOWN),
        vec![press(keys::LEFT), release(keys::LEFT)]
    );
}

#[test]
fn altの対象を右にすると左altは変換しない() {
    let mut config = emacs_active();
    config.emacs.alt_side = SideSelection::Right;
    let mut h = Harness::new(config);
    h.key(keys::LEFTALT, DOWN);
    assert_eq!(
        h.key(keys::KEY_B, DOWN),
        vec![press(keys::LEFTALT), press(keys::KEY_B)]
    );
}

#[test]
fn 押している途中で設定が変わっても押下と離上が対応する() {
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTCTRL, DOWN);
    assert_eq!(
        h.key(keys::KEY_B, DOWN),
        vec![press(keys::LEFT), release(keys::LEFT)]
    );

    let mut config = emacs_active();
    config.emacs.shortcuts.control_b = false;
    h.engine.update_config(config);
    // 捕捉済みのキーは、離すまで握り潰したままにする。
    assert_eq!(h.key(keys::KEY_B, UP), vec![]);
}

#[test]
fn 変換しないと決めたキーは押している間ずっと通す() {
    let mut config = emacs_active();
    config.emacs.shortcuts.control_b = false;
    let mut h = Harness::new(config);
    h.key(keys::LEFTCTRL, DOWN);
    h.key(keys::KEY_B, DOWN);
    assert_eq!(
        h.key(keys::KEY_B, REPEAT),
        vec![Action::Key { code: keys::KEY_B, value: REPEAT }]
    );
    assert_eq!(h.key(keys::KEY_B, UP), vec![release(keys::KEY_B)]);
}

#[test]
fn オートリピートでも変換を繰り返す() {
    let mut h = Harness::new(emacs_active());
    h.key(keys::LEFTCTRL, DOWN);
    h.key(keys::KEY_N, DOWN);
    assert_eq!(
        h.key(keys::KEY_N, REPEAT),
        vec![press(keys::DOWN), release(keys::DOWN)]
    );
}

#[test]
fn 変換中もaltの単押しは影響を受けない() {
    // CapsLock で Emacs モードへ入った直後に Alt を単押しできる。
    let mut config = Config::default();
    config.emacs.enabled = true;
    let mut h = Harness::new(config);
    h.tap(keys::CAPSLOCK);
    h.key(keys::LEFTALT, DOWN);
    h.advance(80);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![Action::Ime(ImeCommand::Off)]);
}

#[test]
fn capslockを押したままのaltはime切り替えに使わない() {
    let mut config = Config::default();
    config.emacs.enabled = true;
    let mut h = Harness::new(config);
    h.key(keys::LEFTALT, DOWN);
    h.key(keys::CAPSLOCK, DOWN);
    h.advance(80);
    assert_eq!(h.key(keys::LEFTALT, UP), vec![]);
    h.key(keys::CAPSLOCK, UP);
}

// ---- 設定ファイル ----

#[test]
fn 設定はtomlで往復できる() {
    let mut config = emacs_active();
    config.emacs.control_side = SideSelection::Right;
    config.emacs.shortcuts.control_k = false;
    config.long_press_ms = 450;
    let text = config.to_toml();
    let parsed: Config = toml::from_str(&text).expect("設定を読み直せませんでした");
    assert_eq!(parsed, config);
}

#[test]
fn 未知のキーが無い設定でも既定値で埋まる() {
    let parsed: Config = toml::from_str("enabled = false\n").expect("設定を読めませんでした");
    assert!(!parsed.enabled);
    assert_eq!(parsed.long_press_ms, 600);
    assert!(parsed.emacs.shortcuts.control_b);
    assert_eq!(parsed.emacs.control_side, SideSelection::Both);
}

#[test]
fn 知らない設定キーを見つけられる() {
    use alt_henkan::config::unknown_keys;

    assert!(unknown_keys(&Config::default().to_toml()).is_empty());
    // 旧書式のキーは今は使われていない。
    assert_eq!(unknown_keys("ime-backend = \"fcitx5\"\n"), vec!["ime-backend"]);
    assert_eq!(
        unknown_keys("[emacs.shortcuts]\ncontrol-z = true\n"),
        vec!["emacs.shortcuts.control-z"]
    );
    assert_eq!(unknown_keys("[ime]\nbakcend = \"ibus\"\n"), vec!["ime.bakcend"]);
    // 壊れた TOML は読み込み側がエラーにするので、ここでは何も言わない。
    assert!(unknown_keys("= broken").is_empty());
}

#[test]
fn 設定を書き戻してもコメントが残る() {
    let existing = "\
# 先頭のコメント
enabled = true

# 長押しの時間
long-press-ms = 600

[emacs.shortcuts]
control-b = true    # 左へ移動
";
    let mut config = Config { enabled: false, long_press_ms: 450, ..Config::default() };
    config.emacs.shortcuts.control_b = false;

    let written = config.to_toml_preserving(existing);
    assert!(written.contains("# 先頭のコメント"), "先頭のコメントが消えた:\n{written}");
    assert!(written.contains("# 長押しの時間"), "キーのコメントが消えた:\n{written}");
    assert!(written.contains("# 左へ移動"), "行末のコメントが消えた:\n{written}");
    assert!(written.contains("enabled = false"), "値が変わっていない:\n{written}");
    assert!(written.contains("long-press-ms = 450"), "値が変わっていない:\n{written}");
    assert!(written.contains("control-b = false"), "値が変わっていない:\n{written}");

    // 書き戻したものを読み直すと同じ設定になる。
    let parsed: Config = toml::from_str(&written).expect("書き戻した設定を読めませんでした");
    assert_eq!(parsed, config);
}

#[test]
fn 元の内容が空なら既定の書式で書き出す() {
    let config = Config::default();
    assert_eq!(config.to_toml_preserving(""), config.to_toml());
    assert_eq!(config.to_toml_preserving("= 壊れている"), config.to_toml());
}
