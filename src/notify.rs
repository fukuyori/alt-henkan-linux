//! モード切り替えの画面表示。Windows 版の ModeOverlayForm に対応する。
//!
//! Wayland にはフォーカスを奪わないオーバーレイを任意のアプリが出す仕組みがないため、
//! 代わりに org.freedesktop.Notifications へ一時通知を出す。replaces_id を使い回すので
//! 通知は積み上がらず、約 1.2 秒で消える。

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::Value;

const TIMEOUT_MS: i32 = 1200;

pub struct ModeNotifier {
    connection: Option<Connection>,
    last_id: u32,
}

impl Default for ModeNotifier {
    fn default() -> Self {
        ModeNotifier::new()
    }
}

impl ModeNotifier {
    pub fn new() -> Self {
        ModeNotifier { connection: Connection::session().ok(), last_id: 0 }
    }

    pub fn show(&mut self, emacs_active: bool) {
        let summary = if emacs_active { "Emacsモード" } else { "通常モード" };
        if let Err(error) = self.notify(summary) {
            crate::diag!("モード表示の通知に失敗しました: {error}");
            self.connection = None;
        }
    }

    fn notify(&mut self, summary: &str) -> zbus::Result<()> {
        if self.connection.is_none() {
            self.connection = Some(Connection::session()?);
        }
        let connection = self.connection.as_ref().expect("接続を確立したはずです");
        let proxy = Proxy::new(
            connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )?;
        let mut hints: HashMap<&str, Value<'_>> = HashMap::new();
        // 履歴に残さず、緊急度も下げる。
        hints.insert("transient", Value::Bool(true));
        hints.insert("urgency", Value::U8(0));
        let id: u32 = proxy.call(
            "Notify",
            &(
                "Alt Henkan",
                self.last_id,
                "input-keyboard",
                summary,
                "",
                Vec::<&str>::new(),
                hints,
                TIMEOUT_MS,
            ),
        )?;
        self.last_id = id;
        Ok(())
    }
}
