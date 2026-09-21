#!/usr/bin/env bash
# Alt Henkan (Linux 版) の削除。設定ファイルは残す。
set -euo pipefail

unit_dir="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"

systemctl --user disable --now alt-henkan.service 2>/dev/null || true
rm -f "${unit_dir}/alt-henkan.service"
systemctl --user daemon-reload
rm -f "${HOME}/.local/bin/alt-henkan" "${HOME}/.local/bin/alt-henkan-settings"
rm -f "${XDG_DATA_HOME:-${HOME}/.local/share}/applications/alt-henkan-settings.desktop"
sudo rm -f /etc/udev/rules.d/99-alt-henkan.rules /etc/modules-load.d/alt-henkan.conf
sudo udevadm control --reload-rules

echo "削除しました。input グループからの除外が必要なら次を実行してください:"
echo "  sudo gpasswd -d ${USER} input"
echo "設定は ${XDG_CONFIG_HOME:-${HOME}/.config}/alt-henkan/ に残しています。"
