#!/bin/bash
# Xcode runs build phase scripts without the user's shell profile, so nvm and Homebrew are not on PATH.
set -eo pipefail
cd "$(dirname "$0")"
if ! command -v node >/dev/null && [ -s "$HOME/.nvm/nvm.sh" ]; then
  . "$HOME/.nvm/nvm.sh"
fi
PATH="$PATH:/opt/homebrew/bin:/usr/local/bin"
if ! command -v node >/dev/null; then
  echo "error: Node.js is needed to build the DiffPane page (web/). Install it with nvm or Homebrew." >&2
  exit 1
fi
if [ ! node_modules/.package-lock.json -nt package-lock.json ]; then
  npm ci --no-audit --no-fund
fi
node build.mjs "$1"
