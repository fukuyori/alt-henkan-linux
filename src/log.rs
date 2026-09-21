//! 診断ログ。既定では警告のみ、`--diagnostics` で全イベントを標準エラーへ出す。
//! systemd のユーザーサービスとして動かすと journald が拾う。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static DIAGNOSTICS: AtomicBool = AtomicBool::new(false);

pub fn set_diagnostics(enabled: bool) {
    DIAGNOSTICS.store(enabled, Ordering::Relaxed);
}

pub fn diagnostics_enabled() -> bool {
    DIAGNOSTICS.load(Ordering::Relaxed)
}

pub fn timestamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = now.as_secs();
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{h:02}:{m:02}:{s:02}.{:03}", now.subsec_millis())
}

/// 常に出すメッセージ。
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        eprintln!("[{}] {}", $crate::log::timestamp(), format_args!($($arg)*))
    };
}

/// `--diagnostics` のときだけ出すメッセージ。通常入力の文字は記録しない。
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => {
        if $crate::log::diagnostics_enabled() {
            eprintln!("[{}] {}", $crate::log::timestamp(), format_args!($($arg)*));
        }
    };
}
