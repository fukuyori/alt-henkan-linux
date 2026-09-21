//! IME のオン・オフ。Windows 版が送っていた VK_IME_ON / VK_IME_OFF に対応する。
//!
//! Linux には IME の状態を直接指定する仮想キーがないため、使っている入力メソッドの
//! 仕組みを直接呼ぶ。
//!
//! - fcitx5: D-Bus (`org.fcitx.Fcitx5` / `/controller` / `org.fcitx.Fcitx.Controller1`)
//!   の Activate・Deactivate。届かないときは `fcitx5-remote -o` / `-c` に退避する。
//! - ibus: `ibus engine <名前>` で入力ソースを切り替える。GNOME で Super+Space を
//!   押したときと同じ操作にあたる。

use std::process::{Command, Stdio};

use zbus::blocking::{Connection, Proxy};

use crate::config::{ImeBackend, ImeSettings};

const FCITX_DESTINATION: &str = "org.fcitx.Fcitx5";
const FCITX_PATH: &str = "/controller";
const FCITX_INTERFACE: &str = "org.fcitx.Fcitx.Controller1";

pub struct Ime {
    settings: ImeSettings,
    connection: Option<Connection>,
    warned: bool,
}

impl Ime {
    pub fn new(settings: ImeSettings) -> Self {
        Ime { settings, connection: Connection::session().ok(), warned: false }
    }

    pub fn configure(&mut self, settings: ImeSettings) {
        if self.settings != settings {
            self.settings = settings;
            self.warned = false;
        }
    }

    pub fn set(&mut self, on: bool) {
        match self.settings.backend {
            ImeBackend::Fcitx5 => self.set_fcitx5(on),
            ImeBackend::Ibus => self.set_ibus(on),
            ImeBackend::None => crate::diag!("IME バックエンドが none なので何もしません。"),
        }
    }

    fn set_fcitx5(&mut self, on: bool) {
        let (method, flag) = if on { ("Activate", "-o") } else { ("Deactivate", "-c") };
        match self.call_fcitx5(method) {
            Ok(()) => {
                crate::diag!("fcitx5: {method} を D-Bus で呼びました。");
                self.warned = false;
            }
            Err(error) => {
                crate::diag!("fcitx5: D-Bus に届きませんでした ({error})。fcitx5-remote を試します。");
                // 接続が切れている可能性があるので、次回は張り直す。
                self.connection = None;
                match run("fcitx5-remote", &[flag]) {
                    Ok(()) => self.warned = false,
                    Err(fallback) => self.warn(format!(
                        "IME を切り替えられませんでした (D-Bus: {error} / fcitx5-remote: {fallback})。\
                         fcitx5 が起動しているか確認してください。"
                    )),
                }
            }
        }
    }

    fn call_fcitx5(&mut self, method: &'static str) -> zbus::Result<()> {
        if self.connection.is_none() {
            self.connection = Some(Connection::session()?);
        }
        let connection = self.connection.as_ref().expect("接続を確立したはずです");
        let proxy = Proxy::new(connection, FCITX_DESTINATION, FCITX_PATH, FCITX_INTERFACE)?;
        proxy.call::<_, _, ()>(method, &())
    }

    fn set_ibus(&mut self, on: bool) {
        let engine = if on {
            self.settings.ibus_engine_on.clone()
        } else {
            self.settings.ibus_engine_off.clone()
        };
        // 既に同じエンジンのときは終了コードが 0 にならないことがあるので、
        // 実際の状態を読んで成否を判断する。
        let _ = run("ibus", &["engine", &engine]);
        match current_ibus_engine() {
            Some(current) if current == engine => {
                crate::diag!("ibus: 入力ソースを {engine} にしました。");
                self.warned = false;
            }
            Some(current) => self.warn(format!(
                "ibus の入力ソースを {engine} にできませんでした (現在は {current})。\
                 `ibus list-engine` に {engine} があるか確認してください。"
            )),
            None => self.warn(
                "ibus に問い合わせできませんでした。ibus-daemon が動いているか確認してください。"
                    .to_owned(),
            ),
        }
    }

    /// 同じ内容を繰り返し出さないようにする。
    fn warn(&mut self, message: String) {
        if !self.warned {
            crate::info!("{message}");
            self.warned = true;
        }
    }
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{program} が {status} で終了しました")),
        Err(error) => Err(format!("{program} を実行できませんでした: {error}")),
    }
}

/// 現在の ibus の入力ソース名。
pub fn current_ibus_engine() -> Option<String> {
    let output = Command::new("ibus").arg("engine").stderr(Stdio::null()).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if name.is_empty() { None } else { Some(name) }
}
