//! 入力スレッドとトレイから本体のループへ渡すメッセージ。

pub enum Message {
    /// キーボードから読んだキーイベント。value は 0=離す 1=押す 2=オートリピート。
    Key { code: u16, value: i32 },
    /// 監視だけしているポインターのボタンかホイール。保留中の Alt / Ctrl を確定させる。
    PointerActivity,
    /// 占有したマウスが見つかった。マウスごとに割り当てが違うので名前を渡す。
    MouseAdded { device: u32, name: String },
    /// 占有したマウスの生イベント。ウィンドウ移動の判定を通して書き戻す。
    Pointer { device: u32, type_: u16, code: u16, value: i32 },
    /// トレイの「有効」。設定ファイルにも保存する。
    SetEnabled(bool),
    /// トレイの「Emacsモード」。現在のモードだけを変え、設定ファイルには保存しない。
    SetEmacsActive(bool),
    /// 設定ウィンドウを開く。
    OpenSettings,
    /// 終了する。
    Quit,
}
