//! 設定ウィンドウ。Windows 版の SettingsForm に対応する。
//!
//! 常駐している本体とは別プロセスで、設定ファイルを書き換えるだけの役割を持つ。
//! 本体は設定ファイルの更新を 2 秒以内に読み直すので、ここから直接指示は送らない。
//! 変更した項目はその場で保存する (OK / キャンセルは無い)。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{Align, Application, ApplicationWindow, Orientation, glib};

use alt_henkan::config::{
    Config, ImeBackend, InitialMode, MAX_LONG_PRESS_MS, MIN_LONG_PRESS_MS, ResizeModifier,
    Shortcuts, WindowMoveButton, WmModifier,
};
use alt_henkan::keys::{Shortcut, SideSelection};

const APP_ID: &str = "io.github.alt_henkan.Settings";

/// 開いている設定と保存先。ウィジェットのコールバックで共有する。
struct State {
    path: PathBuf,
    config: Config,
    status: gtk::Label,
    /// 読み込み中はコールバックからの保存を止める。
    loading: bool,
}

impl State {
    /// 設定を書き換えて保存する。
    fn edit(state: &Rc<RefCell<State>>, change: impl FnOnce(&mut Config)) {
        let mut state = state.borrow_mut();
        if state.loading {
            return;
        }
        change(&mut state.config);
        let config = state.config.clone().normalize();
        state.config = config.clone();
        match config.save(&state.path) {
            Ok(()) => state.status.set_text("保存しました"),
            Err(error) => state.status.set_text(&format!("保存できませんでした: {error}")),
        }
    }
}

fn main() -> glib::ExitCode {
    let path = config_path_from_args();
    let application = Application::builder().application_id(APP_ID).build();
    // --config は自分で解釈するので、GTK には渡さない。
    application.connect_activate(move |app| build(app, path.clone()));
    application.run_with_args::<&str>(&[])
}

fn config_path_from_args() -> PathBuf {
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        if arg == "--config"
            && let Some(value) = argv.next()
        {
            return PathBuf::from(value);
        }
    }
    alt_henkan::config::default_path()
}

fn build(app: &Application, path: PathBuf) {
    let (config, load_error) = match Config::load(&path) {
        Ok(config) => (config, None),
        Err(error) => (Config::default(), Some(error.to_string())),
    };

    let status = gtk::Label::builder().halign(Align::Start).css_classes(["dim-label"]).build();
    status.set_text(&match &load_error {
        Some(error) => format!("設定を読めませんでした ({error})。既定値を表示しています。"),
        None => format!("設定: {}", path.display()),
    });

    let state = Rc::new(RefCell::new(State { path, config, status: status.clone(), loading: true }));

    let notebook = gtk::Notebook::new();
    notebook.append_page(&alt_page(&state), Some(&gtk::Label::new(Some("Alt設定"))));
    notebook.append_page(&emacs_page(&state), Some(&gtk::Label::new(Some("Emacs設定"))));
    notebook.append_page(&window_move_page(&state), Some(&gtk::Label::new(Some("ウィンドウ移動"))));
    notebook.append_page(&display_page(&state), Some(&gtk::Label::new(Some("表示設定"))));

    let root = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();
    root.append(&notebook);
    root.append(&status);

    state.borrow_mut().loading = false;

    ApplicationWindow::builder()
        .application(app)
        .title("Alt Henkan の設定")
        .default_width(520)
        .default_height(560)
        .child(&root)
        .build()
        .present();
}

// ---- 各タブ ----

fn alt_page(state: &Rc<RefCell<State>>) -> gtk::Widget {
    let page = page_box();
    let config = state.borrow().config.clone();

    page.append(&section("Alt キーの動作"));
    page.append(&check(
        "Alt Henkan を有効にする",
        Some("OFF にすると Alt 変換と Emacs 機能の両方を止めます。"),
        config.enabled,
        state,
        |config, value| config.enabled = value,
    ));
    page.append(&check(
        "左 Alt の長押しで本来の Alt を送る",
        None,
        config.left_alt_long_press,
        state,
        |config, value| config.left_alt_long_press = value,
    ));
    page.append(&check(
        "右 Alt の長押しで本来の Alt を送る",
        None,
        config.right_alt_long_press,
        state,
        |config, value| config.right_alt_long_press = value,
    ));

    let spin = gtk::SpinButton::with_range(
        MIN_LONG_PRESS_MS as f64,
        MAX_LONG_PRESS_MS as f64,
        50.0,
    );
    spin.set_value(config.long_press_ms as f64);
    spin.set_halign(Align::Start);
    let for_spin = Rc::clone(state);
    spin.connect_value_changed(move |spin| {
        let value = spin.value() as u64;
        State::edit(&for_spin, |config| config.long_press_ms = value);
    });
    page.append(&labelled("長押しと判定するまでの時間 (ミリ秒)", &spin));

    page.append(&hint(
        "左 Alt の単押しで IME をオフ、右 Alt の単押しで IME をオンにします。\n\
         Alt を押したまま他のキーやマウスを使った場合は、通常の Alt として通します。",
    ));
    scrolled(&page)
}

fn emacs_page(state: &Rc<RefCell<State>>) -> gtk::Widget {
    let page = page_box();
    let config = state.borrow().config.clone();

    page.append(&section("Emacs モード"));
    page.append(&check(
        "CapsLock による Emacs モード切り替えを有効にする",
        Some("OFF のときは CapsLock 本来の大文字固定として動きます。"),
        config.emacs.enabled,
        state,
        |config, value| config.emacs.enabled = value,
    ));

    let modes = ["通常", "Emacs"];
    let initial = gtk::DropDown::from_strings(&modes);
    initial.set_selected(u32::from(config.emacs.initial_mode == InitialMode::Emacs));
    initial.set_halign(Align::Start);
    let for_initial = Rc::clone(state);
    initial.connect_selected_notify(move |drop_down| {
        let mode =
            if drop_down.selected() == 1 { InitialMode::Emacs } else { InitialMode::Normal };
        State::edit(&for_initial, |config| config.emacs.initial_mode = mode);
    });
    page.append(&labelled("起動時のモード", &initial));

    page.append(&side_drop_down("Ctrl の対象", config.emacs.control_side, state, |config, side| {
        config.emacs.control_side = side
    }));
    page.append(&side_drop_down("Alt の対象", config.emacs.alt_side, state, |config, side| {
        config.emacs.alt_side = side
    }));

    page.append(&section("変換する操作"));
    for (shortcut, label) in shortcut_labels() {
        page.append(&check(
            label,
            None,
            config.emacs.shortcuts.is_enabled(shortcut),
            state,
            move |config, value| set_shortcut(&mut config.emacs.shortcuts, shortcut, value),
        ));
    }

    page.append(&hint(
        "ON にした操作は、対象アプリ本来のショートカットより優先します。\n\
         Ctrl+A の全選択などを使いたい場合は、その項目を OFF にしてください。",
    ));
    scrolled(&page)
}

fn window_move_page(state: &Rc<RefCell<State>>) -> gtk::Widget {
    let page = page_box();
    let config = state.borrow().config.clone();

    page.append(&section("ミドルドラッグでウィンドウを動かす"));
    page.append(&check(
        "ウィンドウ移動を有効にする",
        Some("切り替えにはマウスの占有し直しが必要なので、常駐の再起動が要ります。"),
        config.window_move.enabled,
        state,
        |config, value| config.window_move.enabled = value,
    ));

    let buttons = [
        "中ボタン (BTN_MIDDLE)",
        "右ボタン (BTN_RIGHT)",
        "サイド (BTN_SIDE)",
        "拡張 (BTN_EXTRA)",
        "進む (BTN_FORWARD)",
        "戻る (BTN_BACK)",
        "タスク (BTN_TASK)",
        "BTN_0",
    ];
    let button = gtk::DropDown::from_strings(&buttons);
    button.set_selected(match config.window_move.button {
        WindowMoveButton::Middle => 0,
        WindowMoveButton::Right => 1,
        WindowMoveButton::Side => 2,
        WindowMoveButton::Extra => 3,
        WindowMoveButton::Forward => 4,
        WindowMoveButton::Back => 5,
        WindowMoveButton::Task => 6,
        WindowMoveButton::Btn0 => 7,
    });
    button.set_halign(Align::Start);
    let for_button = Rc::clone(state);
    button.connect_selected_notify(move |drop_down| {
        let value = match drop_down.selected() {
            1 => WindowMoveButton::Right,
            2 => WindowMoveButton::Side,
            3 => WindowMoveButton::Extra,
            4 => WindowMoveButton::Forward,
            5 => WindowMoveButton::Back,
            6 => WindowMoveButton::Task,
            7 => WindowMoveButton::Btn0,
            _ => WindowMoveButton::Middle,
        };
        State::edit(&for_button, |config| config.window_move.button = value);
    });
    page.append(&labelled("ドラッグに使うボタン", &button));

    let code = gtk::SpinButton::with_range(0.0, 767.0, 1.0);
    code.set_value(config.window_move.button_code as f64);
    code.set_halign(Align::Start);
    let for_code = Rc::clone(state);
    code.connect_value_changed(move |spin| {
        let value = spin.value() as u16;
        State::edit(&for_code, |config| config.window_move.button_code = value);
    });
    page.append(&labelled("ボタンのコードを直接指定 (0 で使わない)", &code));
    page.append(&hint(
        "一覧に無いボタンは、`alt-henkan --watch-input` で押したときに表示される\n\
         番号をここに入れてください (例: BTN_SIDE (275) なら 275)。",
    ));

    page.append(&section("マウスごとの割り当て"));
    let mice = alt_henkan::devices::connected_mice();
    if mice.is_empty() {
        page.append(&hint(
            "つながっているマウスを読み取れませんでした。\n\
             /dev/input/event* の読み取り権限を確認してください。",
        ));
    } else {
        for (name, buttons) in mice {
            page.append(&device_row(&name, &buttons, state));
        }
        page.append(&hint(
            "マウスによってボタンの構成が違うので、そのマウスが実際に持っている\n\
             ボタンだけを選べるようにしています。「既定に従う」にすると上の設定を使います。",
        ));
    }

    let modifiers = ["Shift", "Ctrl", "Alt", "使わない"];
    let resize = gtk::DropDown::from_strings(&modifiers);
    resize.set_selected(match config.window_move.resize_modifier {
        ResizeModifier::Shift => 0,
        ResizeModifier::Ctrl => 1,
        ResizeModifier::Alt => 2,
        ResizeModifier::None => 3,
    });
    resize.set_halign(Align::Start);
    let for_resize = Rc::clone(state);
    resize.connect_selected_notify(move |drop_down| {
        let value = match drop_down.selected() {
            0 => ResizeModifier::Shift,
            1 => ResizeModifier::Ctrl,
            2 => ResizeModifier::Alt,
            _ => ResizeModifier::None,
        };
        State::edit(&for_resize, |config| config.window_move.resize_modifier = value);
    });
    page.append(&labelled("リサイズに使う修飾キー", &resize));

    let threshold = gtk::SpinButton::with_range(1.0, 64.0, 1.0);
    threshold.set_value(config.window_move.drag_threshold as f64);
    threshold.set_halign(Align::Start);
    let for_threshold = Rc::clone(state);
    threshold.connect_value_changed(move |spin| {
        let value = spin.value() as i32;
        State::edit(&for_threshold, |config| config.window_move.drag_threshold = value);
    });
    page.append(&labelled("ドラッグと判定する移動量 (ピクセル)", &threshold));

    page.append(&section("ウィンドウマネージャーとの連携"));
    let wm_modifiers = ["自動 (GNOME の設定に従う)", "Alt", "Super"];
    let wm = gtk::DropDown::from_strings(&wm_modifiers);
    wm.set_selected(match config.window_move.wm_modifier {
        WmModifier::Auto => 0,
        WmModifier::Alt => 1,
        WmModifier::Super => 2,
    });
    wm.set_halign(Align::Start);
    let for_wm = Rc::clone(state);
    wm.connect_selected_notify(move |drop_down| {
        let value = match drop_down.selected() {
            1 => WmModifier::Alt,
            2 => WmModifier::Super,
            _ => WmModifier::Auto,
        };
        State::edit(&for_wm, |config| config.window_move.wm_modifier = value);
    });
    page.append(&labelled("ドラッグに使う修飾キー", &wm));

    let delay = gtk::SpinButton::with_range(0.0, 200.0, 5.0);
    delay.set_value(config.window_move.modifier_delay_ms as f64);
    delay.set_halign(Align::Start);
    let for_delay = Rc::clone(state);
    delay.connect_value_changed(move |spin| {
        let value = spin.value() as u64;
        State::edit(&for_delay, |config| config.window_move.modifier_delay_ms = value);
    });
    page.append(&labelled("修飾キーを押してからボタンを押すまで (ミリ秒)", &delay));

    page.append(&hint(
        "Wayland には他のアプリのウィンドウを動かす API がないため、ウィンドウ\n\
         マネージャー本来の「修飾キー+ドラッグ」へ翻訳します。GNOME では\n\
         修飾キー+左ドラッグが移動、+中ドラッグがリサイズです。最大化の解除や\n\
         スナップ、最小サイズもウィンドウマネージャーの動作に従います。\n\n\
         占有するのは相対座標のマウス・トラックボールだけで、タッチパッドは対象外です。",
    ));
    scrolled(&page)
}

/// マウス 1 台分の割り当て。そのマウスが持つボタンだけを選択肢にする。
fn device_row(name: &str, buttons: &[u16], state: &Rc<RefCell<State>>) -> gtk::Box {
    use alt_henkan::config::WindowMoveButton;

    let current = state.borrow().config.window_move.rule_for(name).cloned();
    let mut labels = vec!["既定に従う".to_owned()];
    let mut codes: Vec<u16> = Vec::new();
    for code in buttons {
        let label = match WindowMoveButton::from_code(*code) {
            Some(button) => format!("{} ({code})", button_label(button)),
            None => format!("コード {code}"),
        };
        labels.push(label);
        codes.push(*code);
    }
    let current_code = current.as_ref().map(|rule| {
        rule.button_code
            .filter(|code| *code != 0)
            .unwrap_or_else(|| rule.button.unwrap_or_default().code())
    });
    let selected = current_code
        .and_then(|code| codes.iter().position(|candidate| *candidate == code))
        .map_or(0, |index| index + 1);

    let borrowed: Vec<&str> = labels.iter().map(|label| label.as_str()).collect();
    let drop_down = gtk::DropDown::from_strings(&borrowed);
    drop_down.set_selected(selected as u32);
    drop_down.set_halign(Align::Start);

    let device = name.to_owned();
    let for_drop_down = Rc::clone(state);
    drop_down.connect_selected_notify(move |drop_down| {
        let index = drop_down.selected() as usize;
        let code = if index == 0 { None } else { codes.get(index - 1).copied() };
        let device = device.clone();
        State::edit(&for_drop_down, move |config| {
            config.window_move.set_device_button(&device, code)
        });
    });
    labelled(name, &drop_down)
}

fn button_label(button: alt_henkan::config::WindowMoveButton) -> &'static str {
    use alt_henkan::config::WindowMoveButton as B;
    match button {
        B::Middle => "中ボタン",
        B::Right => "右ボタン",
        B::Side => "サイド",
        B::Extra => "拡張",
        B::Forward => "進む",
        B::Back => "戻る",
        B::Task => "タスク",
        B::Btn0 => "BTN_0",
    }
}

fn display_page(state: &Rc<RefCell<State>>) -> gtk::Widget {
    let page = page_box();
    let config = state.borrow().config.clone();

    page.append(&section("表示"));
    page.append(&check(
        "モードを切り替えたときに通知を出す",
        Some("約 1.2 秒で消えます。入力中のウィンドウからフォーカスは移りません。"),
        config.show_mode_notification,
        state,
        |config, value| config.show_mode_notification = value,
    ));

    page.append(&section("IME の切り替え"));
    let backends = ["fcitx5", "ibus", "切り替えない"];
    let backend = gtk::DropDown::from_strings(&backends);
    backend.set_selected(match config.ime.backend {
        ImeBackend::Fcitx5 => 0,
        ImeBackend::Ibus => 1,
        ImeBackend::None => 2,
    });
    backend.set_halign(Align::Start);
    let for_backend = Rc::clone(state);
    backend.connect_selected_notify(move |drop_down| {
        let value = match drop_down.selected() {
            0 => ImeBackend::Fcitx5,
            1 => ImeBackend::Ibus,
            _ => ImeBackend::None,
        };
        State::edit(&for_backend, |config| config.ime.backend = value);
    });
    page.append(&labelled("切り替え方法", &backend));

    page.append(&entry(
        "ibus: IME オンのときの入力ソース",
        &config.ime.ibus_engine_on,
        state,
        |config, value| config.ime.ibus_engine_on = value,
    ));
    page.append(&entry(
        "ibus: IME オフのときの入力ソース",
        &config.ime.ibus_engine_off,
        state,
        |config, value| config.ime.ibus_engine_off = value,
    ));

    page.append(&hint(
        "入力ソース名は `ibus list-engine` で確認できます。\n\
         変更は常駐している本体が 2 秒以内に読み直します。",
    ));
    scrolled(&page)
}

// ---- ウィジェットの組み立て ----

fn page_box() -> gtk::Box {
    gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build()
}

fn scrolled(page: &gtk::Box) -> gtk::Widget {
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(page)
        .build()
        .upcast()
}

fn section(title: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(format!("<b>{title}</b>"))
        .use_markup(true)
        .halign(Align::Start)
        .margin_top(8)
        .build()
}

fn hint(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .halign(Align::Start)
        .wrap(true)
        .margin_top(8)
        .css_classes(["dim-label"])
        .build()
}

fn labelled(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::builder().orientation(Orientation::Horizontal).spacing(12).build();
    let label = gtk::Label::builder().label(title).halign(Align::Start).hexpand(true).build();
    row.append(&label);
    row.append(child);
    row
}

fn check(
    title: &str,
    subtitle: Option<&str>,
    active: bool,
    state: &Rc<RefCell<State>>,
    change: impl Fn(&mut Config, bool) + 'static,
) -> gtk::Box {
    let column = gtk::Box::builder().orientation(Orientation::Vertical).build();
    let button = gtk::CheckButton::with_label(title);
    button.set_active(active);
    let for_button = Rc::clone(state);
    button.connect_toggled(move |button| {
        let value = button.is_active();
        State::edit(&for_button, |config| change(config, value));
    });
    column.append(&button);
    if let Some(subtitle) = subtitle {
        let label = gtk::Label::builder()
            .label(subtitle)
            .halign(Align::Start)
            .wrap(true)
            .margin_start(28)
            .css_classes(["dim-label"])
            .build();
        column.append(&label);
    }
    column
}

fn entry(
    title: &str,
    value: &str,
    state: &Rc<RefCell<State>>,
    change: impl Fn(&mut Config, String) + 'static,
) -> gtk::Box {
    let entry = gtk::Entry::builder().text(value).width_chars(18).build();
    let for_entry = Rc::clone(state);
    entry.connect_changed(move |entry| {
        let value = entry.text().to_string();
        State::edit(&for_entry, |config| change(config, value.clone()));
    });
    labelled(title, &entry)
}

fn side_drop_down(
    title: &str,
    selected: SideSelection,
    state: &Rc<RefCell<State>>,
    change: impl Fn(&mut Config, SideSelection) + 'static,
) -> gtk::Box {
    let drop_down = gtk::DropDown::from_strings(&["両方", "左", "右"]);
    drop_down.set_selected(match selected {
        SideSelection::Both => 0,
        SideSelection::Left => 1,
        SideSelection::Right => 2,
    });
    drop_down.set_halign(Align::Start);
    let for_drop_down = Rc::clone(state);
    drop_down.connect_selected_notify(move |drop_down| {
        let side = match drop_down.selected() {
            1 => SideSelection::Left,
            2 => SideSelection::Right,
            _ => SideSelection::Both,
        };
        State::edit(&for_drop_down, |config| change(config, side));
    });
    labelled(title, &drop_down)
}

fn shortcut_labels() -> [(Shortcut, &'static str); 13] {
    [
        (Shortcut::ControlB, "Ctrl+B：左へ移動"),
        (Shortcut::ControlF, "Ctrl+F：右へ移動"),
        (Shortcut::ControlP, "Ctrl+P：上へ移動"),
        (Shortcut::ControlN, "Ctrl+N：下へ移動"),
        (Shortcut::ControlA, "Ctrl+A：行頭へ移動"),
        (Shortcut::ControlE, "Ctrl+E：行末へ移動"),
        (Shortcut::AltB, "Alt+B：前の単語へ移動"),
        (Shortcut::AltF, "Alt+F：次の単語へ移動"),
        (Shortcut::AltLess, "Alt+<：文書の先頭へ移動"),
        (Shortcut::AltGreater, "Alt+>：文書の末尾へ移動"),
        (Shortcut::ControlD, "Ctrl+D：カーソル位置の文字を削除"),
        (Shortcut::AltD, "Alt+D：次の単語を削除"),
        (Shortcut::ControlK, "Ctrl+K：行末まで削除"),
    ]
}

fn set_shortcut(shortcuts: &mut Shortcuts, shortcut: Shortcut, value: bool) {
    match shortcut {
        Shortcut::ControlB => shortcuts.control_b = value,
        Shortcut::ControlF => shortcuts.control_f = value,
        Shortcut::ControlP => shortcuts.control_p = value,
        Shortcut::ControlN => shortcuts.control_n = value,
        Shortcut::ControlA => shortcuts.control_a = value,
        Shortcut::ControlE => shortcuts.control_e = value,
        Shortcut::AltB => shortcuts.alt_b = value,
        Shortcut::AltF => shortcuts.alt_f = value,
        Shortcut::AltLess => shortcuts.alt_less = value,
        Shortcut::AltGreater => shortcuts.alt_greater = value,
        Shortcut::ControlD => shortcuts.control_d = value,
        Shortcut::AltD => shortcuts.alt_d = value,
        Shortcut::ControlK => shortcuts.control_k = value,
    }
}
