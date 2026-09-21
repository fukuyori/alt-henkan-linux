//! Alt Henkan (Linux 版) の実装。入力変換のロジックは [`engine`] にまとまっていて、
//! OS 呼び出しを含まないので単体でテストできる。

pub mod config;
pub mod desktop;
pub mod devices;
pub mod engine;
pub mod ime;
pub mod keys;
pub mod log;
pub mod message;
pub mod notify;
pub mod tray;
pub mod window_move;
