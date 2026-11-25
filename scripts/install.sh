#!/usr/bin/env bash
# Build diffpilot, install diffpilot.app, and add a `diffpilot` command.
#
#   scripts/install.sh                 install to /Applications (or ~/Applications)
#   scripts/install.sh --dest DIR      install the app into DIR
#   scripts/install.sh --no-cli        skip the `diffpilot` command
#   scripts/install.sh --uninstall     remove the app and the command
set -euo pipefail

APP_NAME="diffpilot"
BUNDLE_ID="com.adamward459.diffpilot"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${DIFFPILOT_BIN_DIR:-$HOME/.local/bin}"
DEST=""
CLI=1
UNINSTALL=0

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
    --no-cli)
        CLI=0
        shift
        ;;
    --uninstall)
        UNINSTALL=1
        shift
        ;;
    -h | --help)
        sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'
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

if ! command -v cargo >/dev/null 2>&1; then
    if [[ -x "$HOME/.cargo/bin/cargo" ]]; then
        PATH="$HOME/.cargo/bin:$PATH"
    else
        fail "Rust is not installed. Install it with:
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
then run this script again."
    fi
fi
command -v git >/dev/null 2>&1 || fail "git is not installed. Run: xcode-select --install"

say "Building release binary (the first build takes a few minutes)"
(cd "$ROOT" && cargo build --release -p diffpilot)
TARGET_DIR="$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
BINARY="$TARGET_DIR/release/$APP_NAME"
[[ -x "$BINARY" ]] || fail "build finished but $BINARY is missing"

VERSION="$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "diffpilot"))')"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
BUNDLE="$STAGE/$APP_NAME.app"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp "$BINARY" "$BUNDLE/Contents/MacOS/$APP_NAME"
cat >"$BUNDLE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$APP_NAME</string>
    <key>CFBundleDisplayName</key><string>$APP_NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key><string>$APP_NAME</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
codesign --force --sign - "$BUNDLE" >/dev/null 2>&1 || say "Ad-hoc signing failed; the app may still run."

say "Installing $APP"
mkdir -p "$DEST"
rm -rf "$APP"
ditto "$BUNDLE" "$APP"

if [[ $CLI -eq 1 ]]; then
    say "Installing the diffpilot command at $SHIM"
    mkdir -p "$BIN_DIR"
    cat >"$SHIM" <<SH
#!/usr/bin/env bash
# Opens a Git repository in diffpilot.app. Installed by diffpilot's scripts/install.sh.
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
