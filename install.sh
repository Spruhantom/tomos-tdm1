#!/usr/bin/env bash
# TomOS Display Manager (TDM) - one-line installer
#
#   curl -fsSL https://raw.githubusercontent.com/Spruhantom/tomos-tdm1/main/install.sh | bash
#
# Options (pass them after "bash -s --", e.g. ... | bash -s -- --yes):
#   --yes           don't ask before switching the login screen
#   --no-enable     install TDM but keep the current login screen
#   --from-source   build from source instead of using the prebuilt release
#   --uninstall     remove TDM and switch back to the previous login screen
#   --help
#
# Works on Tom OS, Ubuntu 24.04+, Debian 13+, Fedora, Arch and openSUSE Tumbleweed (x86_64;
# other CPUs are built from source).
set -euo pipefail

REPO="Spruhantom/tomos-tdm1"
REF="${TDM_REF:-main}"
RELEASE_URL="${TDM_RELEASE_URL:-https://github.com/$REPO/releases/latest/download}"
SOURCE_URL="${TDM_SOURCE_URL:-https://codeload.github.com/$REPO/tar.gz/refs/heads/$REF}"
# For testing: TDM_TARBALL=/path/tdm-x86_64-linux.tar.gz, TDM_SOURCE_DIR=/path/to/checkout
STATE_DIR=/var/lib/tdm
MANIFEST="$STATE_DIR/installed-files"

YES=0 ENABLE=1 FROM_SOURCE=0 ACTION=install
while [ $# -gt 0 ]; do
  case "$1" in
    -y|--yes) YES=1 ;;
    --no-enable) ENABLE=0 ;;
    --from-source) FROM_SOURCE=1 ;;
    --uninstall) ACTION=uninstall ;;
    -h|--help) sed -n '2,15p' "${BASH_SOURCE[0]:-/dev/null}" 2>/dev/null | sed 's/^# \{0,1\}//' || true
               echo "See https://github.com/$REPO"; exit 0 ;;
    *) echo "unknown option: $1 (try --help)" >&2; exit 2 ;;
  esac
  shift
done

if [ -t 2 ] && [ -z "${NO_COLOR:-}" ]; then
  P=$'\e[1;38;5;199m' C=$'\e[1;38;5;51m' G=$'\e[1;32m' Y=$'\e[1;33m' RD=$'\e[1;31m' Z=$'\e[0m'
else P='' C='' G='' Y='' RD='' Z=''; fi
step() { printf '%s==>%s %s\n' "$P" "$Z" "$*" >&2; }
info() { printf '    %s\n' "$*" >&2; }
ok()   { printf '%s✔%s %s\n' "$G" "$Z" "$*" >&2; }
warn() { printf '%swarning:%s %s\n' "$Y" "$Z" "$*" >&2; }
die()  { printf '%serror:%s %s\n' "$RD" "$Z" "$*" >&2; exit 1; }

printf '%s  ⚡ TomOS Display Manager%s  %s- neon login screen for greetd%s\n\n' "$P" "$Z" "$C" "$Z" >&2

[ "$(uname -s)" = Linux ] || die "TDM is a Linux login screen; this computer runs $(uname -s)."

SUDO=""
if [ "$(id -u)" != 0 ]; then
  command -v sudo >/dev/null 2>&1 || die "please run as root (sudo is not installed)"
  SUDO="sudo"
  info "Some steps need administrator rights; you may be asked for your password."
fi
as_root() { $SUDO "$@"; }

ask() {  # ask "question" -> 0 for yes
  [ "$YES" = 1 ] && return 0
  local answer=""
  if [ -r /dev/tty ] && { true </dev/tty; } 2>/dev/null; then
    printf '%s?%s %s [y/N] ' "$C" "$Z" "$1" >/dev/tty
    read -r answer </dev/tty || answer=""
  else
    warn "no terminal to ask \"$1\" - assuming no (use --yes)"
  fi
  case "$answer" in y|Y|yes|YES|Yes) return 0 ;; *) return 1 ;; esac
}

has_systemd() { [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; }

# ---------------------------------------------------------------- distro
PRETTY_NAME=""
# shellcheck disable=SC1091
[ -r /etc/os-release ] && . /etc/os-release
PM=""
for x in apt-get dnf zypper pacman; do command -v "$x" >/dev/null 2>&1 && { PM=$x; break; }; done

pkg_install() {  # pkg_install pkg...
  [ $# -gt 0 ] || return 0
  case "$PM" in
    apt-get) as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$@" ;;
    dnf)     as_root dnf install -y "$@" ;;
    zypper)  as_root zypper --non-interactive install --no-recommends "$@" ;;
    pacman)  as_root pacman -S --needed --noconfirm "$@" ;;
    *) return 1 ;;
  esac
}
pkg_refresh() {
  case "$PM" in
    apt-get) as_root apt-get update -qq ;;
    pacman)  as_root pacman -Sy --noconfirm >/dev/null ;;
    *) : ;;
  esac
}

runtime_pkgs() {
  case "$PM" in
    apt-get) echo "greetd cage libgtk-4-1 polkitd ca-certificates fonts-jetbrains-mono" ;;
    dnf)     echo "greetd cage gtk4 polkit jetbrains-mono-fonts" ;;
    zypper)  echo "greetd cage gtk4-tools libgtk-4-1 polkit jetbrains-mono-fonts" ;;
    pacman)  echo "greetd cage gtk4 polkit ttf-jetbrains-mono-nerd" ;;
  esac
}
build_pkgs() {
  case "$PM" in
    apt-get) echo "build-essential pkg-config libgtk-4-dev curl" ;;
    dnf)     echo "gcc pkgconf-pkg-config gtk4-devel curl" ;;
    zypper)  echo "gcc pkg-config gtk4-devel curl" ;;
    pacman)  echo "base-devel pkgconf gtk4 curl" ;;
  esac
}

# ---------------------------------------------------------------- uninstall
if [ "$ACTION" = uninstall ]; then
  step "Removing TDM"
  if has_systemd; then
    prev=$(as_root cat "$STATE_DIR/previous-dm" 2>/dev/null || true)
    if [ -n "$prev" ] && [ "$prev" != greetd.service ]; then
      as_root systemctl disable greetd.service >/dev/null 2>&1 || true
      as_root systemctl enable --force "$prev" >/dev/null 2>&1 && info "Login screen switched back to $prev (next boot)" ||
        warn "could not re-enable $prev - enable your login screen with: sudo systemctl enable --force <name>.service"
    elif [ -z "$prev" ]; then
      warn "no previous login screen recorded; greetd is left enabled"
    fi
  fi
  if as_root test -f "$MANIFEST"; then
    as_root cat "$MANIFEST" | while read -r f; do
      [ -n "$f" ] || continue
      if as_root test -f "$f.before-tdm"; then as_root mv -f "$f.before-tdm" "$f"; info "restored $f"
      else as_root rm -f "$f"; fi
    done
  fi
  as_root rm -rf "$STATE_DIR"
  ok "TDM removed. Your settings in /etc/tdm were kept (delete them with: sudo rm -r /etc/tdm)."
  exit 0
fi

# ---------------------------------------------------------------- install
WORK=$(mktemp -d "${TMPDIR:-/tmp}/tdm-install.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

step "Installing what TDM needs (greetd, cage, GTK 4)"
if [ -n "$PM" ]; then
  info "using $PM on ${PRETTY_NAME:-this system}"
  pkg_refresh || warn "package list refresh failed"
  # shellcheck disable=SC2046
  if ! pkg_install $(runtime_pkgs); then
    warn "installing all packages failed - retrying without the optional font"
    # shellcheck disable=SC2046
    pkg_install $(runtime_pkgs | tr ' ' '\n' | grep -v -i -E 'font|ttf' | tr '\n' ' ') ||
      die "could not install greetd / cage / GTK 4. On Ubuntu, enable the 'universe' repository first."
  fi
else
  warn "unknown package manager - make sure greetd, cage and GTK 4.12+ are installed"
fi
command -v greetd >/dev/null 2>&1 || [ -x /usr/sbin/greetd ] || die "greetd is not installed"
command -v cage >/dev/null 2>&1 || die "cage is not installed"

binaries_work() {  # binaries_work DIR - LD_BIND_NOW makes a too-old GTK fail here, not at login
  LD_BIND_NOW=1 "$1/tdm" --version >/dev/null 2>&1 && LD_BIND_NOW=1 "$1/tdm-greeter" --version >/dev/null 2>&1
}

SRC=""   # directory with tdm, tdm-greeter, data/
if [ "$FROM_SOURCE" = 0 ] && [ "$(uname -m)" = x86_64 ]; then
  step "Downloading TDM"
  tarball="$WORK/tdm.tar.gz"
  if [ -n "${TDM_TARBALL:-}" ]; then cp "$TDM_TARBALL" "$tarball"; cp "$TDM_TARBALL.sha256" "$tarball.sha256" 2>/dev/null || true
  else
    curl -fsSL --retry 3 -o "$tarball" "$RELEASE_URL/tdm-x86_64-linux.tar.gz" &&
      curl -fsSL --retry 3 -o "$tarball.sha256" "$RELEASE_URL/tdm-x86_64-linux.tar.gz.sha256" || {
        warn "no prebuilt release found - building from source instead"; rm -f "$tarball"; }
  fi
  if [ -f "$tarball" ]; then
    if [ -f "$tarball.sha256" ]; then
      want=$(cut -d' ' -f1 < "$tarball.sha256"); have=$(sha256sum "$tarball" | cut -d' ' -f1)
      [ "$want" = "$have" ] || die "download is damaged (SHA-256 mismatch) - try again"
      info "SHA-256 checked"
    fi
    mkdir -p "$WORK/bin" && tar -xzf "$tarball" -C "$WORK/bin" --strip-components=1
    if binaries_work "$WORK/bin"; then SRC="$WORK/bin"; info "prebuilt $("$WORK/bin/tdm" --version 2>/dev/null)"
    else warn "the prebuilt TDM doesn't run here (GTK 4.12+ / glibc 2.39+ needed) - building from source"; fi
  fi
fi

if [ -z "$SRC" ]; then
  step "Building TDM from source (a few minutes)"
  # shellcheck disable=SC2046
  pkg_install $(build_pkgs) || warn "could not install build tools"
  if [ -n "${TDM_SOURCE_DIR:-}" ]; then cp -r "$TDM_SOURCE_DIR" "$WORK/src"
  else
    mkdir -p "$WORK/src"
    curl -fsSL --retry 3 "$SOURCE_URL" | tar -xz -C "$WORK/src" --strip-components=1 || die "could not download the source"
  fi
  cargo_ok() {  # cargo 1.83 or newer
    command -v cargo >/dev/null 2>&1 || return 1
    local v maj min; v=$(cargo --version | cut -d' ' -f2); maj=${v%%.*}; min=${v#*.}; min=${min%%.*}
    [ "$maj" -gt 1 ] 2>/dev/null || { [ "$maj" = 1 ] && [ "$min" -ge 83 ] 2>/dev/null; }
  }
  if ! cargo_ok; then
    info "installing a private Rust toolchain (rustup) for the build"
    export RUSTUP_HOME="$WORK/rustup" CARGO_HOME="$WORK/cargo"
    curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path >/dev/null 2>&1 ||
      die "could not install Rust"
    export PATH="$CARGO_HOME/bin:$PATH"
  fi
  (cd "$WORK/src" && CARGO_TARGET_DIR="$WORK/target" cargo build --release --locked) ||
    die "build failed - GTK 4.12 or newer is required"
  mkdir -p "$WORK/bin"
  cp "$WORK/target/release/tdm" "$WORK/target/release/tdm-greeter" "$WORK/bin/"
  cp -r "$WORK/src/data" "$WORK/bin/data"
  binaries_work "$WORK/bin" || die "the freshly built TDM doesn't start"
  SRC="$WORK/bin"
fi

# The greetd user is _greetd on Debian/Ubuntu and greeter on Arch/Fedora/openSUSE.
GUSER=""
if [ -f /etc/greetd/config.toml ]; then
  GUSER=$(sed -n 's/^[[:space:]]*user[[:space:]]*=[[:space:]]*"\{0,1\}\([A-Za-z0-9_.-]*\)"\{0,1\}.*/\1/p' /etc/greetd/config.toml | head -n1)
  id "$GUSER" >/dev/null 2>&1 || GUSER=""
fi
if [ -z "$GUSER" ]; then
  for u in _greetd greeter greetd; do id "$u" >/dev/null 2>&1 && { GUSER=$u; break; }; done
fi
if [ -z "$GUSER" ]; then
  GUSER=_greetd
  info "creating the system user $GUSER"
  as_root useradd --system --no-create-home --home-dir /var/lib/greetd --shell /usr/sbin/nologin "$GUSER" 2>/dev/null ||
    as_root useradd -r -M -d /var/lib/greetd -s /sbin/nologin "$GUSER"
fi
# Let the greeter use the GPU like other display managers do
for grp in video render input; do getent group "$grp" >/dev/null 2>&1 && as_root usermod -aG "$grp" "$GUSER" 2>/dev/null || true; done

step "Installing TDM"
as_root mkdir -p "$STATE_DIR"
: > "$WORK/manifest"
ours() {  # ours FILE - installed by an earlier run of this script
  as_root test -f "$MANIFEST" && as_root grep -qxF "$1" "$MANIFEST"
}
put() {  # put MODE SRC DEST [keep|settings]
  # keep:     leave an existing file alone (e.g. a wallpaper that's already there)
  # settings: like keep, and never removed by --uninstall (the user's greeter.conf)
  local mode="$1" from="$2" to="$3" kind="${4:-}"
  if [ -n "$kind" ] && as_root test -e "$to"; then
    [ "$kind" = settings ] && info "kept your $to"
    return 0
  fi
  if as_root test -f "$to" && ! as_root test -f "$to.before-tdm" && ! ours "$to" &&
     ! as_root grep -qs 'tdm-greeter' "$to"; then
    as_root cp -p "$to" "$to.before-tdm"; info "backed up $to"
  fi
  as_root install -Dm"$mode" "$from" "$to"
  [ "$kind" = settings ] || echo "$to" >> "$WORK/manifest"
}
sed "s/_greetd/$GUSER/g" "$SRC/data/greetd-config.toml" > "$WORK/greetd.toml"
sed "s/_greetd/$GUSER/g" "$SRC/data/tdm.tmpfiles"       > "$WORK/tdm.tmpfiles"
sed "s/_greetd/$GUSER/g" "$SRC/data/50-tdm-greeter.rules" > "$WORK/polkit.rules"

put 755 "$SRC/tdm"                 /usr/bin/tdm
put 755 "$SRC/tdm-greeter"         /usr/bin/tdm-greeter
put 755 "$SRC/data/tomos-dm"       /usr/bin/tomos-dm
put 644 "$WORK/greetd.toml"        /etc/greetd/config.toml
put 644 "$SRC/data/greeter.conf"   /etc/tdm/greeter.conf settings
put 644 "$WORK/tdm.tmpfiles"       /usr/lib/tmpfiles.d/tdm.conf
put 644 "$WORK/polkit.rules"       /etc/polkit-1/rules.d/50-tdm-greeter.rules
put 644 "$SRC/data/logo.png"       /usr/share/tdm/logo.png
put 644 "$SRC/data/wallpaper-eclipse.png" /usr/share/backgrounds/tomos/wallpaper-eclipse.png keep
as_root sh -c "cat '$WORK/manifest' >> '$MANIFEST' && sort -u -o '$MANIFEST' '$MANIFEST'"

if command -v systemd-tmpfiles >/dev/null 2>&1; then as_root systemd-tmpfiles --create /usr/lib/tmpfiles.d/tdm.conf || true
else as_root install -d -m750 -o "$GUSER" -g "$(id -gn "$GUSER")" "$STATE_DIR"; fi
info "greeter runs as: $GUSER"
ver=$(/usr/bin/tdm --version 2>/dev/null || true); ok "TDM ${ver##* } installed"

# ---------------------------------------------------------------- enable
if [ "$ENABLE" = 1 ]; then
  if has_systemd; then
    cur=$(basename "$(readlink -f /etc/systemd/system/display-manager.service 2>/dev/null || true)")
    [ "$cur" = display-manager.service ] && cur=""
    if [ "$cur" = greetd.service ]; then
      info "greetd is already your login screen - TDM will show at the next login"
    elif ask "Use TDM as your login screen from the next boot?${cur:+ (replaces $cur)}"; then
      as_root sh -c "printf '%s\n' '${cur:-}' > '$STATE_DIR/previous-dm'"
      [ -n "$cur" ] && as_root systemctl disable "$cur" >/dev/null 2>&1 || true
      as_root systemctl enable --force greetd.service >/dev/null 2>&1 || die "could not enable greetd.service"
      ok "TDM will be your login screen after a reboot${cur:+ (was $cur)}"
    else
      info "Kept your current login screen. Switch later with: sudo systemctl enable --force greetd.service"
    fi
  else
    warn "systemd is not running here, so the login screen was not switched"
  fi
fi

echo >&2
/usr/bin/tdm check 2>&1 | sed 's/^/    /' >&2 || true
cat >&2 <<EOF

${C}Next:${Z}
    reboot to see TDM.  Customise it in /etc/tdm/greeter.conf
    guide:   https://github.com/$REPO/blob/main/docs/CUSTOMISING.md
    preview: TDM_WINDOWED=1 tdm-greeter
    remove:  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install.sh | bash -s -- --uninstall
EOF
