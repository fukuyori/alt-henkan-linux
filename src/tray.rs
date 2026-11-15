//! 通知領域のアイコンとメニュー。Windows 版の TrayApplicationContext に対応する。
//!
//! StatusNotifierItem (KDE/freedesktop 仕様) として登録する。GNOME では
//! AppIndicator 系の拡張機能が有効になっていれば上部バーに表示される。

use std::sync::mpsc::Sender;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{MenuItem, OfflineReason, ToolTip};

use crate::message::Message;

pub struct Tray {
    sender: Sender<Message>,
    /// 設定の「有効」。
    pub enabled: bool,
    /// Emacs 機能そのものが設定で有効か。
    pub emacs_available: bool,
    /// 今 Emacs モードか。
    pub emacs_active: bool,
}

impl Tray {
    pub fn new(
        sender: Sender<Message>,
        enabled: bool,
        emacs_available: bool,
        emacs_active: bool,
    ) -> Self {
        Tray { sender, enabled, emacs_available, emacs_active }
    }

    fn status_text(&self) -> String {
        if !self.enabled {
            "無効".to_owned()
        } else if !self.emacs_available {
            "通常モード".to_owned()
        } else if self.emacs_active {
            "Emacsモード".to_owned()
        } else {
            "通常モード".to_owned()
        }
    }

    fn send(&self, message: Message) {
        // 本体が終了していてもメニュー操作で落とさない。
        let _ = self.sender.send(message);
    }
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "alt-henkan".into()
    }

    fn title(&self) -> String {
        "Alt Henkan".into()
    }

    fn icon_name(&self) -> String {
        // 無効のときも同じアイコンにして、状態は説明文とメニューで示す。
        "input-keyboard".into()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Alt Henkan".into(),
            description: self.status_text(),
            icon_name: "input-keyboard".into(),
            icon_pixmap: Vec::new(),
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: format!("状態: {}", self.status_text()),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "有効".into(),
                checked: self.enabled,
                activate: Box::new(|tray: &mut Tray| {
                    tray.send(Message::SetEnabled(!tray.enabled));
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Emacsモード".into(),
                checked: self.emacs_active,
                // Emacs 機能が設定で無効なら選べない。
                enabled: self.enabled && self.emacs_available,
                activate: Box::new(|tray: &mut Tray| {
                    tray.send(Message::SetEmacsActive(!tray.emacs_active));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "設定...".into(),
                icon_name: "preferences-system".into(),
                activate: Box::new(|tray: &mut Tray| tray.send(Message::OpenSettings)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "終了".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray: &mut Tray| tray.send(Message::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // アイコンのクリックで設定を開く (Windows 版のダブルクリックに相当)。
        self.send(Message::OpenSettings);
    }

    fn watcher_online(&self) {
        crate::info!("通知領域が使えるようになりました。アイコンを表示します。");
    }

    fn watcher_offline(&self, reason: OfflineReason) -> bool {
        match reason {
            OfflineReason::Error(error) => crate::info!(
                "通知領域のアイコンをまだ表示できません ({error})。\
                 GNOME では AppIndicator 系の拡張機能が必要です。使えるようになるまで待ちます。"
            ),
            _ => crate::info!("通知領域が無くなりました。戻るまで待ちます。"),
        }
        // 止めずに待ち、通知領域が現れたら ksni に登録し直させる。
        true
    }
}

/// トレイを登録する。D-Bus に繋がらないなど登録の見込みが無ければ `None` を返し、
/// 本体は常駐を続ける。
///
/// ログイン直後は GNOME の拡張機能より先に起動することがあるので、通知領域がまだ
/// 無いだけなら失敗にせず、現れた時点で登録する。
pub fn spawn(tray: Tray) -> Option<Handle<Tray>> {
    match tray.assume_sni_available(true).spawn() {
        Ok(handle) => Some(handle),
        Err(error) => {
            crate::info!(
                "通知領域のアイコンを表示できません ({error})。常駐は続けます。"
            );
            None
        }
    }
}
