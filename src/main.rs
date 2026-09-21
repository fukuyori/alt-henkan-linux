//! Alt Henkan (Linux 版) — 左右の Alt キーの単押しに IME のオン・オフを割り当てる常駐プログラム。

use std::path::PathBuf;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant, SystemTime};

use evdev::uinput::VirtualDevice;
use evdev::{EventType, InputEvent};

use alt_henkan::config::{self, Config, WmModifier};
use alt_henkan::desktop::{Desktop, Request};
use alt_henkan::devices::{self, DeviceWatcher};
use alt_henkan::message::Message;
use alt_henkan::tray::{self, Tray};
use alt_henkan::engine::{Action, Engine, ImeCommand};
use alt_henkan::window_move::{Frame, Out, WindowMove};
use alt_henkan::{diag, info, keys, log};

/// 設定ファイルの更新を見にいく間隔。
const CONFIG_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const USAGE: &str = "\
Alt Henkan — 左右の Alt キーの単押しで IME を切り替える常駐プログラム

使い方:
  alt-henkan [オプション]

オプション:
  --config <パス>   設定ファイルを指定する (既定: $XDG_CONFIG_HOME/alt-henkan/config.toml)
  --diagnostics     Alt の処理・モード切り替え・Emacs 操作を標準エラーへ記録する
                    (通常入力の文字は記録しない)
  --list-devices    検出したキーボードとポインターを一覧表示して終了する
  --watch-input     押したキー・ボタンの名前をその場で表示する (Ctrl+C で終了)
  --test-ime        IME のオフ・オンを実際に送って切り替えを確認する
  --print-config    読み込んだ設定を TOML で表示して終了する
  --write-config    設定ファイルが無ければ既定値で作成して終了する
  --help            このヘルプを表示する
  --version         バージョンを表示する
";

struct Args {
    config_path: PathBuf,
    diagnostics: bool,
    command: Command,
}

enum Command {
    Run,
    ListDevices,
    WatchInput,
    PrintConfig,
    WriteConfig,
    TestIme,
}

fn parse_args() -> anyhow::Result<Option<Args>> {
    let mut config_path = None;
    let mut diagnostics = false;
    let mut command = Command::Run;
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--config" => {
                let value = argv.next().ok_or_else(|| anyhow::anyhow!("--config にパスがありません"))?;
                config_path = Some(PathBuf::from(value));
            }
            "--diagnostics" => diagnostics = true,
            "--list-devices" => command = Command::ListDevices,
            "--watch-input" => command = Command::WatchInput,
            "--test-ime" => command = Command::TestIme,
            "--print-config" => command = Command::PrintConfig,
            "--write-config" => command = Command::WriteConfig,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "--version" | "-V" => {
                println!("alt-henkan {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            other => anyhow::bail!("不明なオプション: {other}\n\n{USAGE}"),
        }
    }
    Ok(Some(Args {
        config_path: config_path.unwrap_or_else(config::default_path),
        diagnostics,
        command,
    }))
}

fn main() -> anyhow::Result<()> {
    let Some(args) = parse_args()? else {
        return Ok(());
    };
    log::set_diagnostics(args.diagnostics);

    match args.command {
        Command::ListDevices => return list_devices(),
        Command::WatchInput => return watch_input(),
        Command::PrintConfig => {
            let config = Config::load(&args.config_path)?;
            print!("{}", config.to_toml());
            return Ok(());
        }
        Command::WriteConfig => {
            if args.config_path.exists() {
                info!("{} は既にあります。", args.config_path.display());
            } else {
                Config::default().save(&args.config_path)?;
                info!("{} を作成しました。", args.config_path.display());
            }
            return Ok(());
        }
        Command::TestIme => {
            // 失敗した理由がそのまま見えるように診断を強制的に有効にする。
            log::set_diagnostics(true);
            return test_ime(&Config::load(&args.config_path)?);
        }
        Command::Run => {}
    }

    run(args)
}

fn list_devices() -> anyhow::Result<()> {
    let mut found = 0;
    for (path, device) in evdev::enumerate() {
        let role = if devices::is_keyboard(&device) {
            "キーボード (占有)"
        } else if devices::is_pointer(&device) {
            "ポインター (監視)"
        } else {
            continue;
        };
        println!("{:<20} {:<18} {}", path.display(), role, device.name().unwrap_or("(名前なし)"));
        found += 1;
    }
    if found == 0 {
        // 権限が無いデバイスは enumerate が黙って飛ばすので、その可能性を伝える。
        println!(
            "対象のデバイスが見つかりませんでした。/dev/input/event* の読み取り権限を確認してください \
             (id -nG に input が含まれているか、dist/install.sh を実行したか)。"
        );
    }
    Ok(())
}

/// IME の切り替えだけを試す。キーボードの占有も uinput も使わない。
fn test_ime(config: &Config) -> anyhow::Result<()> {
    use alt_henkan::config::ImeBackend;
    use alt_henkan::ime::Ime;

    println!("バックエンド: {:?}", config.ime.backend);
    if config.ime.backend == ImeBackend::None {
        println!("ime.backend が \"none\" なので切り替えません。");
        return Ok(());
    }
    let mut ime = Ime::new(config.ime.clone());
    println!("切り替え前: {}", ime_state(config));
    for (label, on) in [("左 Alt 単押し (IME オフ)", false), ("右 Alt 単押し (IME オン)", true)] {
        println!("\n{label}");
        ime.set(on);
        std::thread::sleep(Duration::from_millis(800));
        println!("  -> {}", ime_state(config));
    }
    Ok(())
}

/// 今の IME の状態を人が読める形で返す。
fn ime_state(config: &Config) -> String {
    use alt_henkan::config::ImeBackend;
    match config.ime.backend {
        ImeBackend::Ibus => alt_henkan::ime::current_ibus_engine()
            .unwrap_or_else(|| "(ibus に問い合わせできません)".to_owned()),
        ImeBackend::Fcitx5 => match std::process::Command::new("fcitx5-remote").output() {
            Ok(output) => match String::from_utf8_lossy(&output.stdout).trim() {
                // fcitx5 は D-Bus 活性化で起動するので、呼び出しが成功しても
                // アプリ側が fcitx5 を使っていなければ 0 のまま変わらない。
                "0" => "入力コンテキストがありません \
                        (アプリが fcitx5 を使っていません。XMODIFIERS などを確認してください)"
                    .to_owned(),
                "1" => "非アクティブ (IME オフ)".to_owned(),
                "2" => "アクティブ (IME オン)".to_owned(),
                "" => "(fcitx5-remote が応答しません)".to_owned(),
                other => format!("fcitx5-remote: {other}"),
            },
            Err(error) => format!("(fcitx5-remote を実行できません: {error})"),
        },
        ImeBackend::None => "(未使用)".to_owned(),
    }
}

/// 押したキー・ボタンの名前をその場で表示する。デバイスは占有しない。
fn watch_input() -> anyhow::Result<()> {
    use evdev::KeyCode;

    let daemon_running = evdev::enumerate()
        .any(|(_, device)| device.name().is_some_and(|name| name.contains("alt-henkan virtual")));
    if daemon_running {
        println!(
            "注意: alt-henkan が動いています。占有しているデバイスのイベントはここに届きません。\n\
             \x20     いったん止めてから試してください:\n\
             \x20       systemctl --user stop alt-henkan.service\n\
             \x20       alt-henkan --watch-input\n\
             \x20       systemctl --user start alt-henkan.service\n"
        );
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    let mut seen = std::collections::HashSet::new();
    // 起動後に挿したデバイスも拾えるよう、巡回して追加する。
    let watch_new = |seen: &mut std::collections::HashSet<PathBuf>| -> usize {
        let mut added = 0;
        for (path, mut device) in evdev::enumerate() {
            if seen.contains(&path)
                || (!devices::is_keyboard(&device) && !devices::is_pointer(&device))
            {
                continue;
            }
            let name = device.name().unwrap_or("(名前なし)").to_owned();
            println!("監視中: {name} ({})", path.display());
            seen.insert(path.clone());
            added += 1;
            let sender = sender.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("watch-{}", path.display()))
                .spawn(move || {
                    while let Ok(events) = device.fetch_events() {
                        for event in events {
                            let message =
                                (name.clone(), event.event_type(), event.code(), event.value());
                            if sender.send(message).is_err() {
                                return;
                            }
                        }
                    }
                });
            if let Err(error) = spawned {
                println!("  読み取りを開始できませんでした: {error}");
            }
        }
        added
    };

    if watch_new(&mut seen) == 0 {
        anyhow::bail!(
            "デバイスを開けませんでした。/dev/input/event* の読み取り権限を確認してください。"
        );
    }
    println!("\nキーやボタンを押してください。Ctrl+C で終了します。\n");

    loop {
        let (name, event_type, code, value) =
            match receiver.recv_timeout(Duration::from_millis(1000)) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => {
                    watch_new(&mut seen);
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            };
        let line = match event_type {
            EventType::KEY => {
                let state = match value {
                    0 => "離した",
                    1 => "押した",
                    _ => "押しっぱなし",
                };
                format!("{:?} ({code})  {state}", KeyCode::new(code))
            }
            EventType::RELATIVE if devices::is_wheel(code) => {
                format!("{:?} ({code})  {value:+}", evdev::RelativeAxisCode(code))
            }
            _ => continue,
        };
        println!("{name:<38} {line}");
    }
    Ok(())
}

fn run(args: Args) -> anyhow::Result<()> {
    let config = Config::load(&args.config_path)?;
    info!("設定: {}", args.config_path.display());
    diag!("設定内容:\n{}", config.to_toml());

    let mut sink = devices::create_sink().map_err(|error| {
        anyhow::anyhow!(
            "仮想キーボードを作れませんでした ({error})。\
             /dev/uinput への書き込み権限があるか確認してください (dist/install.sh を参照)。"
        )
    })?;

    let (sender, receiver) = channel();
    let sender_for_tray = sender.clone();
    // マウスを占有するかは起動時に決める (途中で掴み直せないため)。
    let grab_mice = config.window_move.enabled;
    let mut watcher = DeviceWatcher::new(sender, grab_mice);
    if watcher.scan() == 0 {
        anyhow::bail!(
            "入力デバイスを 1 つも開けませんでした。/dev/input/event* の読み取り権限を確認してください \
             (dist/install.sh を参照)。"
        );
    }

    let mut engine = Engine::new(config);
    let mut current_config = engine.config().clone();
    // 以降のデバイスの抜き差しは専用スレッドが追いかける。
    watcher.run_in_background();

    let desktop = Desktop::spawn(current_config.ime.clone());

    // ウィンドウ移動を使うときだけ仮想ポインターを作る。
    let mut pointer_sink = if grab_mice {
        match devices::create_pointer_sink() {
            Ok(sink) => Some(sink),
            Err(error) => {
                info!("仮想ポインターを作れませんでした ({error})。ウィンドウ移動は使えません。");
                None
            }
        }
    } else {
        None
    };
    let wm_modifier = wm_modifier_key(&current_config);
    // マウスごとにボタンの構成が違うので、1 台につき 1 つ状態を持つ。
    let mut window_moves: std::collections::HashMap<u32, WindowMove> =
        std::collections::HashMap::new();
    if grab_mice {
        info!(
            "ウィンドウ移動: 有効 (修飾キー {})",
            if wm_modifier == keys::LEFTMETA { "Super" } else { "Alt" }
        );
    }
    info!(
        "起動しました。初期モード: {}",
        if engine.emacs_active() { "Emacs" } else { "通常" }
    );

    // CapsLock を Emacs モードの切り替えに使うので、大文字固定が残っていたら解除する。
    if current_config.emacs_enabled() && devices::caps_lock_on() {
        emit_key(&mut sink, keys::CAPSLOCK, 1);
        emit_key(&mut sink, keys::CAPSLOCK, 0);
        info!("大文字固定を解除しました。");
    }

    let tray_handle = tray::spawn(Tray::new(
        sender_for_tray,
        current_config.enabled,
        current_config.emacs.enabled,
        engine.emacs_active(),
    ));
    // トレイの表示を今の状態に合わせる。
    let refresh_tray = |handle: &Option<ksni::blocking::Handle<Tray>>,
                        config: &Config,
                        emacs_active: bool| {
        if let Some(handle) = handle {
            handle.update(|tray| {
                tray.enabled = config.enabled;
                tray.emacs_available = config.emacs.enabled;
                tray.emacs_active = emacs_active;
            });
        }
    };

    // 1 フレーム分のポインターイベントを貯める場所。
    let mut pointer_frame = Frame::new();
    let started = Instant::now();
    let mut last_scan = Instant::now();
    let mut config_stamp = file_stamp(&args.config_path);

    loop {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(Message::Key { code, value }) => {
                let now_ms = started.elapsed().as_millis() as u64;
                let actions = engine.on_key(code, value, now_ms);
                let mode_changed =
                    actions.iter().any(|a| matches!(a, Action::ModeChanged { .. }));
                apply(&mut sink, &desktop, &current_config, actions);
                if mode_changed {
                    refresh_tray(&tray_handle, &current_config, engine.emacs_active());
                }
            }
            Ok(Message::PointerActivity) => {
                let actions = engine.on_pointer_activity();
                apply(&mut sink, &desktop, &current_config, actions);
            }
            Ok(Message::MouseAdded { device, name }) => {
                let mover = WindowMove::for_device(
                    current_config.window_move.clone(),
                    wm_modifier,
                    &name,
                );
                info!(
                    "ウィンドウ移動: {name} は {:?} ({}) でドラッグします",
                    evdev::KeyCode::new(mover.trigger()),
                    mover.trigger()
                );
                window_moves.insert(device, mover);
            }
            Ok(Message::Pointer { device, type_, code, value }) => {
                // 占有したマウスのボタン・ホイールも、保留中の Alt / Ctrl を確定させる。
                if (type_ == EventType::KEY.0 && devices::is_pointer_button(code) && value != 0)
                    || (type_ == EventType::RELATIVE.0 && devices::is_wheel(code))
                {
                    let actions = engine.on_pointer_activity();
                    apply(&mut sink, &desktop, &current_config, actions);
                }
                if type_ == EventType::SYNCHRONIZATION.0 {
                    // 元のフレームの区切り。ここまでを 1 フレームとして書き出す。
                    flush_pointer(&mut pointer_sink, &mut pointer_frame);
                } else if let Some(mover) = window_moves.get_mut(&device) {
                    let modifier_down = window_move_modifier_down(&engine, &current_config);
                    let out = mover.on_event(type_, code, value, modifier_down);
                    queue_pointer(
                        &mut pointer_sink,
                        &mut pointer_frame,
                        mover.modifier_delay_ms(),
                        out,
                    );
                }
            }
            Ok(Message::SetEnabled(enabled)) => {
                let updated = Config { enabled, ..current_config.clone() };
                // 設定ファイルへも残す。書き込めなくても動作は変えない。
                if let Err(error) = updated.save(&args.config_path) {
                    info!("設定を保存できませんでした: {error}");
                }
                config_stamp = file_stamp(&args.config_path);
                current_config = updated.clone();
                let actions = engine.update_config(updated);
                apply(&mut sink, &desktop, &current_config, actions);
                info!("{}", if enabled { "有効にしました。" } else { "無効にしました。" });
                refresh_tray(&tray_handle, &current_config, engine.emacs_active());
            }
            Ok(Message::SetEmacsActive(active)) => {
                let actions = engine.set_emacs_active(active);
                apply(&mut sink, &desktop, &current_config, actions);
                refresh_tray(&tray_handle, &current_config, engine.emacs_active());
            }
            Ok(Message::OpenSettings) => open_settings(&args.config_path),
            Ok(Message::Quit) => {
                info!("終了します。");
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        // 設定ファイルの更新だけ見る。デバイスの巡回は専用スレッドが受け持つ。
        if last_scan.elapsed() >= CONFIG_CHECK_INTERVAL {
            last_scan = Instant::now();
            let stamp = file_stamp(&args.config_path);
            if stamp != config_stamp {
                config_stamp = stamp;
                match Config::load(&args.config_path) {
                    Ok(reloaded) => {
                        info!("設定を読み直しました。");
                        current_config = reloaded.clone();
                        desktop.send(Request::Configure(current_config.ime.clone()));
                        if current_config.window_move.enabled != grab_mice {
                            info!(
                                "ウィンドウ移動の有効・無効を変えました。マウスの占有を切り替えるには\
                                 常駐を再起動してください: systemctl --user restart alt-henkan.service"
                            );
                        }
                        for mover in window_moves.values_mut() {
                            let out = mover
                                .update_config(current_config.window_move.clone(), wm_modifier);
                            queue_pointer(
                                &mut pointer_sink,
                                &mut pointer_frame,
                                mover.modifier_delay_ms(),
                                out,
                            );
                        }
                        flush_pointer(&mut pointer_sink, &mut pointer_frame);
                        let actions = engine.update_config(reloaded);
                        apply(&mut sink, &desktop, &current_config, actions);
                        refresh_tray(&tray_handle, &current_config, engine.emacs_active());
                    }
                    Err(error) => info!("設定を読み直せませんでした: {error}"),
                }
            }
        }
    }

    for mover in window_moves.values_mut() {
        let out = mover.cancel();
        queue_pointer(&mut pointer_sink, &mut pointer_frame, mover.modifier_delay_ms(), out);
    }
    flush_pointer(&mut pointer_sink, &mut pointer_frame);
    let actions = engine.release_everything();
    apply(&mut sink, &desktop, &current_config, actions);
    Ok(())
}

/// ウィンドウマネージャーが「修飾キー+ドラッグ」に使っているキー。
fn wm_modifier_key(config: &Config) -> u16 {
    match config.window_move.wm_modifier {
        WmModifier::Alt => keys::LEFTALT,
        WmModifier::Super => keys::LEFTMETA,
        WmModifier::Auto => detect_wm_modifier().unwrap_or(keys::LEFTALT),
    }
}

/// GNOME の設定から読む。読めないときは None。
fn detect_wm_modifier() -> Option<u16> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.wm.preferences", "mouse-button-modifier"])
        .output()
        .ok()?;
    let value = String::from_utf8_lossy(&output.stdout).to_lowercase();
    if value.contains("super") {
        Some(keys::LEFTMETA)
    } else if value.contains("alt") {
        Some(keys::LEFTALT)
    } else {
        None
    }
}

/// リサイズ用の修飾キーが物理的に押されているか。
fn window_move_modifier_down(engine: &Engine, config: &Config) -> bool {
    alt_henkan::window_move::resize_modifier_keys(config.window_move.resize_modifier)
        .iter()
        .any(|code| engine.is_key_down(*code))
}

/// ウィンドウ移動の出力を、組み立て中のフレームへ積む。
///
/// 元のマウスは X と Y の移動を 1 フレームにまとめて送ってくる。1 件ずつ
/// SYN_REPORT を付けて書き出すと 2 回の動きに割れてしまい、カーソルの動きが
/// がたつく。ここでは SYN_REPORT が来るまで貯めて、まとめて書き出す。
fn queue_pointer(
    sink: &mut Option<VirtualDevice>,
    frame: &mut Frame,
    settle_ms: u64,
    out: Vec<Out>,
) {
    for action in out {
        match action {
            Out::Event { type_, code, value } => {
                if let Some(ready) = frame.push(type_, code, value) {
                    write_frame(sink, ready);
                }
            }
            // 修飾キーをウィンドウマネージャーが認識してからボタンを送る。
            Out::Settle => {
                flush_pointer(sink, frame);
                std::thread::sleep(Duration::from_millis(settle_ms));
            }
        }
    }
}

/// 貯めたイベントを 1 フレームとして書き出す。
fn flush_pointer(sink: &mut Option<VirtualDevice>, frame: &mut Frame) {
    if !frame.is_empty() {
        write_frame(sink, frame.take());
    }
}

fn write_frame(sink: &mut Option<VirtualDevice>, events: Vec<(u16, u16, i32)>) {
    let Some(sink) = sink else {
        return;
    };
    let batch: Vec<InputEvent> = events
        .into_iter()
        .map(|(type_, code, value)| InputEvent::new(type_, code, value))
        .collect();
    // emit が末尾に SYN_REPORT を 1 つだけ付ける。
    if let Err(error) = sink.emit(&batch) {
        info!("ポインターの書き出しに失敗しました: {error}");
    }
}

fn apply(sink: &mut VirtualDevice, desktop: &Desktop, config: &Config, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Key { code, value } => emit_key(sink, code, value),
            Action::Ime(command) => {
                diag!("IME: {}", if command == ImeCommand::On { "オン" } else { "オフ" });
                desktop.send(match command {
                    ImeCommand::On => Request::ImeOn,
                    ImeCommand::Off => Request::ImeOff,
                });
            }
            Action::Emacs(label) => diag!("Emacs操作: {label}"),
            Action::ModeChanged { emacs_active } => {
                info!("モード: {}", if emacs_active { "Emacs" } else { "通常" });
                if config.show_mode_notification {
                    desktop.send(Request::ModeChanged(emacs_active));
                }
            }
        }
    }
}

/// 設定ウィンドウを別プロセスで開く。自分の隣にある実行ファイルを優先して探す。
fn open_settings(config_path: &std::path::Path) {
    const PROGRAM: &str = "alt-henkan-settings";
    let neighbour = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.join(PROGRAM)))
        .filter(|path| path.is_file());
    let program = neighbour.unwrap_or_else(|| PathBuf::from(PROGRAM));
    // systemd-run で別のスコープに出すと、常駐を再起動しても設定ウィンドウが
    // 道連れにならず、終了を待たずに済むので子プロセスも残らない。
    let detached = std::process::Command::new("systemd-run")
        .args(["--user", "--quiet", "--collect", "--scope", "--"])
        .arg(&program)
        .arg("--config")
        .arg(config_path)
        .spawn();
    let spawned = match detached {
        Ok(child) => Ok(child),
        Err(error) => {
            diag!("systemd-run を使えませんでした ({error})。そのまま起動します。");
            std::process::Command::new(&program).arg("--config").arg(config_path).spawn()
        }
    };
    match spawned {
        Ok(mut child) => {
            diag!("設定ウィンドウを開きました: {}", program.display());
            // 終了を待たないが、残骸を残さないよう別スレッドで回収する。
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(error) => info!(
            "設定ウィンドウを開けませんでした ({error})。\
             {} が入っているか確認してください。",
            program.display()
        ),
    }
}

fn emit_key(sink: &mut VirtualDevice, code: u16, value: i32) {
    // 1 イベントごとに SYN_REPORT を伴わせる。押下と離上を同じフレームに入れると消える。
    if let Err(error) = sink.emit(&[InputEvent::new(EventType::KEY.0, code, value)]) {
        info!("キーの書き出しに失敗しました (code={code}, value={value}): {error}");
    }
}

fn file_stamp(path: &std::path::Path) -> Option<(SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}
