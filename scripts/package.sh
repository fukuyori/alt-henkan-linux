#!/usr/bin/env bash
# deb パッケージを作る。
#   ビルド → パッケージの中身を組み立てる → target/package/ へ .deb を出力
#
# 使い方:
#   ./scripts/package.sh              # target/package/ へ出力する
#   ./scripts/package.sh 出力先       # 出力先を変える
#
# Maintainer 欄は DEB_MAINTAINER="名前 <メールアドレス>" で指定できる。
#
# dist/install.sh はホームディレクトリへ入れるが、deb は /usr へ入れる。
#   /usr/bin/                        alt-henkan, alt-henkan-settings
#   /usr/lib/systemd/user/           alt-henkan.service
#   /usr/lib/udev/rules.d/           99-alt-henkan.rules
#   /usr/lib/modules-load.d/         alt-henkan.conf (uinput)
#   /usr/share/applications/         alt-henkan-settings.desktop
#   /usr/share/alt-henkan/           setup-fcitx5.sh, im-fcitx5-env.service
#   /usr/share/doc/alt-henkan/       README.md, images/, config.example.toml, copyright
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="${1:-${repo}/target/package}"
# 普段の target/ とは別にビルドし、開発用のビルドを巻き込まない。
build_dir="${repo}/target/package-build"

say() { printf '\n== %s\n' "$*"; }

for tool in cargo dpkg-deb dpkg; do
  if ! command -v "${tool}" >/dev/null; then
    echo "${tool} が見つかりません。" >&2
    exit 1
  fi
done

package=alt-henkan
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "${repo}/Cargo.toml" | head -n 1)"
arch="$(dpkg --print-architecture)"
if [[ -z "${version}" ]]; then
  echo "Cargo.toml からバージョンを取得できません。" >&2
  exit 1
fi

if [[ -n "${DEB_MAINTAINER:-}" ]]; then
  maintainer="${DEB_MAINTAINER}"
else
  # 配る物に個人のメールアドレスを入れないよう、既定では GitHub の noreply を使う。
  git_user="$(git -C "${repo}" config user.name 2>/dev/null || true)"
  git_user="${git_user:-alt-henkan}"
  maintainer="${git_user} <${git_user}@users.noreply.github.com>"
fi

say "${package} ${version} (${arch}) をビルドします"
# パニック時のメッセージなどにホームディレクトリのパスが残らないよう置き換える。
cargo_home="${CARGO_HOME:-${HOME}/.cargo}"
RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=${cargo_home}=/cargo --remap-path-prefix=${repo}=/src" \
  CARGO_TARGET_DIR="${build_dir}" \
  cargo build --release --locked --manifest-path "${repo}/Cargo.toml"

say "パッケージの中身を組み立てます"
stage="$(mktemp -d)"
trap 'rm -rf "${stage}"' EXIT
root="${stage}/root"

install -D -m 0755 "${build_dir}/release/alt-henkan" "${root}/usr/bin/alt-henkan"
install -D -m 0755 "${build_dir}/release/alt-henkan-settings" "${root}/usr/bin/alt-henkan-settings"
strip "${root}/usr/bin/alt-henkan" "${root}/usr/bin/alt-henkan-settings"

# ユーザーサービス。配置先が /usr/bin になるので ExecStart を書き換える。
# install.sh と違って設定ディレクトリを先に作らないので、systemd に作らせる
# (無いと ReadWritePaths の指定で起動に失敗する)。
unit="${root}/usr/lib/systemd/user/alt-henkan.service"
install -D -m 0644 "${repo}/dist/alt-henkan.service" "${unit}"
sed -i \
  -e 's|^ExecStart=.*|ExecStart=/usr/bin/alt-henkan|' \
  -e 's|^ReadWritePaths=\(.*\)|ConfigurationDirectory=alt-henkan\nReadWritePaths=\1|' \
  "${unit}"
if ! grep -qx 'ExecStart=/usr/bin/alt-henkan' "${unit}" \
  || ! grep -qx 'ConfigurationDirectory=alt-henkan' "${unit}"; then
  echo "dist/alt-henkan.service を書き換えられません。書式が変わっていないか確認してください。" >&2
  exit 1
fi

install -D -m 0644 "${repo}/dist/99-alt-henkan.rules" \
  "${root}/usr/lib/udev/rules.d/99-alt-henkan.rules"
# uinput はモジュールとして提供されることが多いので、起動時に読み込ませる。
install -d "${root}/usr/lib/modules-load.d"
echo uinput > "${root}/usr/lib/modules-load.d/alt-henkan.conf"
chmod 0644 "${root}/usr/lib/modules-load.d/alt-henkan.conf"

install -D -m 0644 "${repo}/dist/alt-henkan-settings.desktop" \
  "${root}/usr/share/applications/alt-henkan-settings.desktop"

# fcitx5 用の設定スクリプトは、同じ場所のユニットファイルを参照するので並べて置く。
install -D -m 0755 "${repo}/dist/setup-fcitx5.sh" "${root}/usr/share/alt-henkan/setup-fcitx5.sh"
install -D -m 0644 "${repo}/dist/im-fcitx5-env.service" \
  "${root}/usr/share/alt-henkan/im-fcitx5-env.service"

doc="${root}/usr/share/doc/${package}"
install -D -m 0644 "${repo}/README.md" "${doc}/README.md"
install -D -m 0644 "${repo}/dist/config.example.toml" "${doc}/config.example.toml"
install -D -m 0644 "${repo}/LICENSE" "${doc}/copyright"
install -d "${doc}/images"
install -m 0644 "${repo}"/images/* "${doc}/images/"

# 依存パッケージ。dpkg-shlibdeps があれば、リンクしているライブラリから求める。
depends="libc6, libgtk-4-1"
if command -v dpkg-shlibdeps >/dev/null; then
  mkdir -p "${stage}/shlibs/debian"
  printf 'Source: %s\n\nPackage: %s\nArchitecture: any\n' "${package}" "${package}" \
    > "${stage}/shlibs/debian/control"
  if found="$(cd "${stage}/shlibs" && dpkg-shlibdeps -O \
      -e "${root}/usr/bin/alt-henkan" -e "${root}/usr/bin/alt-henkan-settings" 2>/dev/null)"; then
    found="${found#shlibs:Depends=}"
    [[ -n "${found}" ]] && depends="${found}"
  fi
fi

installed_size="$(du -sk "${root}" | cut -f1)"

mkdir -p "${root}/DEBIAN"
cat > "${root}/DEBIAN/control" <<EOF
Package: ${package}
Version: ${version}
Architecture: ${arch}
Maintainer: ${maintainer}
Installed-Size: ${installed_size}
Depends: ${depends}
Suggests: gnome-shell-extension-appindicator, fcitx5 | ibus
Section: utils
Priority: optional
Homepage: https://github.com/fukuyori/alt-henkan-linux
Description: 左右の Alt キーの単押しで IME を切り替える常駐プログラム
 左 Alt の単押しで IME をオフ、右 Alt の単押しでオンにする。Emacs 風の
 キー操作と、Alt を押しながらのウィンドウ移動にも対応する。
 evdev と uinput で動くので、Wayland でも X11 でも使える。
EOF

cat > "${root}/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e

if [ "$1" = configure ]; then
  # /dev/uinput を input グループへ開放する udev ルールを反映する。
  modprobe uinput 2>/dev/null || true
  if command -v udevadm >/dev/null; then
    udevadm control --reload-rules 2>/dev/null || true
    udevadm trigger --subsystem-match=input --subsystem-match=misc 2>/dev/null || true
  fi

  # sudo で入れた利用者を input グループへ追加する (入力デバイスの読み取りに必要)。
  user="${SUDO_USER:-}"
  added=0
  if [ -n "$user" ] && [ "$user" != root ] && id "$user" >/dev/null 2>&1; then
    if ! id -nG "$user" | tr ' ' '\n' | grep -qx input; then
      usermod -aG input "$user"
      added=1
    fi
  fi

  echo
  echo "Alt Henkan を入れました。"
  if [ "$added" = 1 ]; then
    echo "  $user を input グループへ追加しました。一度ログアウトして入り直してください。"
  elif [ -z "$user" ] || [ "$user" = root ]; then
    echo "  使う利用者を input グループへ追加してください: sudo usermod -aG input <ユーザー名>"
  fi
  echo "  常駐させるには、使う利用者で次を実行します。"
  echo "    systemctl --user daemon-reload"
  echo "    systemctl --user enable --now alt-henkan.service"
  echo "  更新した場合は systemctl --user restart alt-henkan.service で新しい版に切り替わります。"
fi
EOF

cat > "${root}/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e

if [ "$1" = remove ] || [ "$1" = purge ]; then
  if command -v udevadm >/dev/null; then
    udevadm control --reload-rules 2>/dev/null || true
  fi
fi
EOF
chmod 0755 "${root}/DEBIAN/postinst" "${root}/DEBIAN/postrm"

say ".deb を作ります"
mkdir -p "${out_dir}"
deb="${out_dir}/${package}_${version}_${arch}.deb"
dpkg-deb --root-owner-group --build "${root}" "${deb}" >/dev/null

say "完了しました"
echo "${deb}"
echo "インストール: sudo apt install ${deb}"
