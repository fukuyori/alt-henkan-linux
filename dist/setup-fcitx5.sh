#!/usr/bin/env bash
# GNOME Wayland で fcitx5 を既定の入力メソッドにする。
#
# このディストリビューションの im-config は Wayland セッションでは何もしない
# (/etc/profile.d/im-config_wayland.sh の中身が丸ごとコメントアウトされている)。
# そのため `im-config -n fcitx5` を実行しても、GNOME が自分の入力ソース設定に従って
# ibus を起動し直し、再起動のたびに ibus へ戻る。ここでは次の 3 つを直接設定する。
#
#   1. GNOME の入力ソースから ibus のものを外す (gnome-shell が ibus を起動しなくなる)
#   2. 入力メソッドの環境変数を ~/.config/environment.d/ で固定する
#   3. gnome-shell が XMODIFIERS と QT_IM_MODULE を ibus で上書きするので、
#      セッションの初期化後に設定し直すユーザーサービスを入れる
#   4. fcitx5 を自動起動に登録する
#
# 元に戻すときは --revert を付けて実行する。
set -euo pipefail

conf_dir="${XDG_CONFIG_HOME:-${HOME}/.config}"
env_file="${conf_dir}/environment.d/60-im-fcitx5.conf"
unit_file="${conf_dir}/systemd/user/im-fcitx5-env.service"
autostart="${conf_dir}/autostart/org.fcitx.Fcitx5.desktop"
backup="${conf_dir}/alt-henkan/input-sources.backup"
source_desktop=/usr/share/applications/org.fcitx.Fcitx5.desktop

say() { printf '\n== %s\n' "$*"; }

if [[ "${1:-}" == "--revert" ]]; then
  say "元に戻します"
  systemctl --user disable --now im-fcitx5-env.service 2>/dev/null || true
  rm -f "${env_file}" "${unit_file}" "${autostart}"
  systemctl --user daemon-reload
  if [[ -e "${backup}" ]]; then
    gsettings set org.gnome.desktop.input-sources sources "$(cat "${backup}")"
    echo "入力ソースを $(cat "${backup}") に戻しました。"
    rm -f "${backup}"
  else
    echo "入力ソースの控えが無いので変更しません。GNOME の設定から追加し直してください。"
  fi
  echo "ログアウトして入り直すと反映されます。"
  exit 0
fi

command -v fcitx5 >/dev/null || {
  echo "fcitx5 が見つかりません。先に導入してください:" >&2
  echo "  sudo apt install fcitx5 fcitx5-mozc fcitx5-config-qt" >&2
  exit 1
}

say "GNOME の入力ソースから ibus を外します"
mkdir -p "$(dirname "${backup}")"
current="$(gsettings get org.gnome.desktop.input-sources sources)"
if [[ ! -e "${backup}" ]]; then
  printf '%s' "${current}" > "${backup}"
  echo "現在の設定を ${backup} に控えました: ${current}"
  if [[ "${current}" != *"'ibus'"* ]]; then
    # 控えに ibus のソースが無いと --revert で日本語入力が戻らない。
    echo "注意: 現在の入力ソースに ibus のものがありません。"
    echo "      以前に外している場合、この控えでは日本語入力を戻せません。"
    echo "      本来の値が分かるなら ${backup} を直接書き換えてください。"
  fi
else
  echo "控えは ${backup} に既にあります: $(cat "${backup}")"
fi
filtered="$(python3 - "${current}" <<'PY'
import ast, sys
# GVariant の "[('xkb', 'us'), ('ibus', 'mozc-jp')]" は Python のリテラルとして読める。
sources = ast.literal_eval(sys.argv[1])
kept = [entry for entry in sources if entry[0] != "ibus"]
print(kept or [("xkb", "us")])
PY
)"
gsettings set org.gnome.desktop.input-sources sources "${filtered}"
echo "入力ソースを ${filtered} にしました。"

say "入力メソッドの環境変数を設定します"
mkdir -p "$(dirname "${env_file}")"
cat > "${env_file}" <<'EOF'
# alt-henkan の dist/setup-fcitx5.sh が作成。削除すると元の挙動に戻る。
GTK_IM_MODULE=fcitx
QT_IM_MODULE=fcitx
XMODIFIERS=@im=fcitx
SDL_IM_MODULE=fcitx
EOF
echo "${env_file} を作成しました。"

say "環境変数を設定し直すユーザーサービスを入れます"
mkdir -p "$(dirname "${unit_file}")"
install -m 0644 "$(dirname "${BASH_SOURCE[0]}")/im-fcitx5-env.service" "${unit_file}"
systemctl --user daemon-reload
systemctl --user enable --now im-fcitx5-env.service
echo "${unit_file} を登録しました。"

say "fcitx5 を自動起動に登録します"
mkdir -p "$(dirname "${autostart}")"
if [[ -e "${source_desktop}" ]]; then
  install -m 0644 "${source_desktop}" "${autostart}"
else
  cat > "${autostart}" <<'EOF'
[Desktop Entry]
Type=Application
Name=Fcitx 5
Exec=fcitx5
X-GNOME-Autostart-Phase=Applications
EOF
fi
echo "${autostart} を作成しました。"

cat <<'MSG'

== 完了しました
ログアウトして入り直してください。その後の確認方法:

  echo $XMODIFIERS          # @im=fcitx になっていること
                            # (ターミナルを開き直してから確認すること)
  fcitx5-remote             # 1 (オフ) か 2 (オン) が返ること
  fcitx5-configtool         # 「日本語 - Mozc」を入力メソッドに追加する

注意:
  GNOME (Mutter) は Wayland の input-method プロトコルに対応していないため、
  fcitx5 は GTK / Qt の入力モジュールと、Xwayland アプリ向けの XIM 経由で動きます。
  ほとんどの GTK・Qt アプリでは使えますが、一部のアプリ (GNOME Shell の検索など) では
  日本語入力ができません。ibus のまま使いたい場合は、alt-henkan の設定を
  ime.backend = "ibus" にすれば、そのままでも Alt での切り替えが使えます。

  元に戻すには: ./dist/setup-fcitx5.sh --revert
MSG
