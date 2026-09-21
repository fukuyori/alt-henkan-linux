#!/usr/bin/env bash
# Alt Henkan (Linux 版) のインストール。
#   ビルド → ~/.local/bin へ配置 → uinput の権限設定 (sudo) → ユーザーサービス登録
# 権限設定だけ sudo を使う。常駐プロセス自体は通常権限で動く。
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin_dir="${HOME}/.local/bin"
desktop_dir="${XDG_DATA_HOME:-${HOME}/.local/share}/applications"
unit_dir="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
config_dir="${XDG_CONFIG_HOME:-${HOME}/.config}/alt-henkan"

say() { printf '\n== %s\n' "$*"; }

say "ビルドします"
cargo build --release --manifest-path "${repo}/Cargo.toml"

say "${bin_dir} へ配置します"
mkdir -p "${bin_dir}"
install -m 0755 "${repo}/target/release/alt-henkan" "${bin_dir}/alt-henkan"
install -m 0755 "${repo}/target/release/alt-henkan-settings" "${bin_dir}/alt-henkan-settings"

say "設定ウィンドウをアプリ一覧に登録します"
mkdir -p "${desktop_dir}"
install -m 0644 "${repo}/dist/alt-henkan-settings.desktop" \
  "${desktop_dir}/alt-henkan-settings.desktop"
update-desktop-database "${desktop_dir}" 2>/dev/null || true

say "入力デバイスの権限を設定します (sudo が必要です)"
sudo install -m 0644 "${repo}/dist/99-alt-henkan.rules" /etc/udev/rules.d/99-alt-henkan.rules
# uinput はモジュールとして提供されることが多いので、起動時に読み込ませる。
echo uinput | sudo tee /etc/modules-load.d/alt-henkan.conf >/dev/null
sudo modprobe uinput || true
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=input --subsystem-match=misc

if ! id -nG "${USER}" | tr ' ' '\n' | grep -qx input; then
  say "${USER} を input グループへ追加します"
  sudo usermod -aG input "${USER}"
  needs_relogin=1
else
  needs_relogin=0
fi

say "設定ファイルを用意します"
mkdir -p "${config_dir}"
if [[ -e "${config_dir}/config.toml" ]]; then
  echo "${config_dir}/config.toml は既にあるので変更しません。"
else
  install -m 0644 "${repo}/dist/config.example.toml" "${config_dir}/config.toml"
  echo "${config_dir}/config.toml を作成しました。"
fi

say "ユーザーサービスを登録します"
mkdir -p "${unit_dir}"
install -m 0644 "${repo}/dist/alt-henkan.service" "${unit_dir}/alt-henkan.service"
systemctl --user daemon-reload
systemctl --user enable alt-henkan.service

say "完了しました"
if [[ "${needs_relogin}" == "1" ]]; then
  cat <<'MSG'
input グループを追加したので、一度ログアウトして入り直してください。
その後、次のコマンドで起動します。

  systemctl --user start alt-henkan.service
  systemctl --user status alt-henkan.service
MSG
else
  echo "次のコマンドで起動します: systemctl --user start alt-henkan.service"
fi
