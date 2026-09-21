//! デスクトップ側への出力 (IME 切り替えとモード表示) をまとめた作業スレッド。
//!
//! D-Bus 呼び出しは数ミリ秒かかることがあるので、入力処理のスレッドからは切り離す。
//! Windows 版が診断ログと通知をフックから切り離していたのと同じ意図。

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use crate::config::ImeSettings;
use crate::ime::Ime;
use crate::notify::ModeNotifier;

pub enum Request {
    ImeOn,
    ImeOff,
    ModeChanged(bool),
    /// 設定を読み直したときに、IME の切り替え方法を差し替える。
    Configure(ImeSettings),
}

#[derive(Clone)]
pub struct Desktop {
    sender: Sender<Request>,
}

impl Desktop {
    pub fn spawn(settings: ImeSettings) -> Desktop {
        let (sender, receiver) = channel();
        thread::Builder::new()
            .name("alt-henkan-desktop".into())
            .spawn(move || run(receiver, settings))
            .expect("作業スレッドを起動できませんでした");
        Desktop { sender }
    }

    pub fn send(&self, request: Request) {
        // 受け手が落ちていても入力処理は続ける。
        let _ = self.sender.send(request);
    }
}

fn run(receiver: Receiver<Request>, settings: ImeSettings) {
    let mut ime = Ime::new(settings);
    let mut notifier = ModeNotifier::new();
    while let Ok(request) = receiver.recv() {
        match request {
            Request::ImeOn => ime.set(true),
            Request::ImeOff => ime.set(false),
            Request::ModeChanged(active) => notifier.show(active),
            Request::Configure(settings) => ime.configure(settings),
        }
    }
}
