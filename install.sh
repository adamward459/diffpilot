#!/usr/bin/env bash
# Install diffpilot.app from the latest GitHub release and add a `diffpilot` command.
#
#   install.sh                     install to /Applications (or ~/Applications)
#   install.sh --version v0.2.0    install that release instead of the latest
#   install.sh --from-source       build from this clone instead of downloading
#   install.sh --dest DIR          install the app into DIR
#   install.sh --no-cli            skip the `diffpilot` command
#   install.sh --uninstall         remove the app and the command
set -euo pipefail

APP_NAME="diffpilot"
REPO="adamward459/diffpilot"
BIN_DIR="${DIFFPILOT_BIN_DIR:-$HOME/.local/bin}"
DEST=""
CLI=1
UNINSTALL=0
FROM_SOURCE=0
VERSION=""

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

while [[ $# -gt 0 ]]; do
    case "$1" in
    --dest)
        DEST="${2:?--dest needs a folder}"
        shift 2
        ;;
    --version)
        VERSION="${2:?--version needs a tag, e.g. v0.2.0}"
        shift 2
        ;;
    --from-source)
        FROM_SOURCE=1
        shift
        ;;
    --no-cli)
        CLI=0
        shift
        ;;
    --uninstall)
        UNINSTALL=1
        shift
        ;;
    -h | --help)
        sed -n '2,10p' "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *) fail "unknown option: $1 (try --help)" ;;
    esac
done

[[ "$(uname -s)" == "Darwin" ]] || fail "diffpilot runs on macOS only."

if [[ -z "$DEST" ]]; then
    if [[ -w /Applications ]]; then DEST="/Applications"; else DEST="$HOME/Applications"; fi
fi
APP="$DEST/$APP_NAME.app"
SHIM="$BIN_DIR/$APP_NAME"

if [[ $UNINSTALL -eq 1 ]]; then
    for target in "$APP" "$HOME/Applications/$APP_NAME.app" "/Applications/$APP_NAME.app"; do
        if [[ -d "$target" ]]; then
            say "Removing $target"
            rm -rf "$target"
        fi
    done
    if [[ -f "$SHIM" ]] && grep -q "diffpilot.app" "$SHIM"; then
        say "Removing $SHIM"
        rm -f "$SHIM"
    fi
    say "Settings stay in ~/Library/Application Support/diffpilot. Delete that folder to reset them."
    exit 0
fi

command -v git >/dev/null 2>&1 || fail "git is not installed. Run: xcode-select --install"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

if [[ $FROM_SOURCE -eq 1 ]]; then
    ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    [[ -f "$ROOT/Cargo.toml" ]] || fail "--from-source needs a clone: git clone https://github.com/$REPO.git"
    if ! command -v cargo >/dev/null 2>&1; then
        if [[ -x "$HOME/.cargo/bin/cargo" ]]; then
            PATH="$HOME/.cargo/bin:$PATH"
        else
            fail "Rust is not installed. Install it with:
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
then run this script again."
        fi
    fi
    say "Building release binary (the first build takes a few minutes)"
    BUNDLE="$(cd "$ROOT" && cargo xtask bundle | tail -1)"
else
    if [[ -n "$VERSION" ]]; then
        URL="https://github.com/$REPO/releases/download/$VERSION/diffpilot-macos.zip"
    else
        URL="https://github.com/$REPO/releases/latest/download/diffpilot-macos.zip"
    fi
    say "Downloading $URL"
    curl -fsSL "$URL" -o "$STAGE/diffpilot.zip" ||
        fail "download failed. Check the version, or build it yourself with --from-source."
    ditto -x -k "$STAGE/diffpilot.zip" "$STAGE"
    BUNDLE="$STAGE/$APP_NAME.app"
fi
[[ -x "$BUNDLE/Contents/MacOS/$APP_NAME" ]] || fail "$BUNDLE is not a diffpilot app"

say "Installing $APP"
mkdir -p "$DEST"
rm -rf "$APP"
ditto "$BUNDLE" "$APP"
# The app is ad-hoc signed, so Gatekeeper would block it if it kept the quarantine flag.
xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true

if [[ $CLI -eq 1 ]]; then
    say "Installing the diffpilot command at $SHIM"
    mkdir -p "$BIN_DIR"
    cat >"$SHIM" <<SH
#!/usr/bin/env bash
# Opens a Git repository in diffpilot.app. Installed by diffpilot's install.sh.
set -euo pipefail
folder="\$(cd "\${1:-.}" && pwd)"
exec open -n -a "$APP" --args "\$folder"
SH
    chmod +x "$SHIM"
    case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) say "Add $BIN_DIR to your PATH to use the diffpilot command, e.g. in ~/.zshrc: export PATH=\"$BIN_DIR:\$PATH\"" ;;
    esac
fi

say "Done. Open diffpilot from $DEST, or run: diffpilot /path/to/repo"
