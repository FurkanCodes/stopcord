#!/bin/bash
# Installs the latest Stopcord release into /Applications:
#   curl -fsSL https://raw.githubusercontent.com/FurkanCodes/stopcord/main/scripts/install.sh | bash
# Downloads the zip, checks its SHA-256, and asks whether macOS should check
# the app before its first launch. Read it before running it.
#
# While the repo is private, install with the GitHub CLI signed in instead:
#   gh api repos/FurkanCodes/stopcord/contents/scripts/install.sh -H 'Accept: application/vnd.github.raw' | bash
set -euo pipefail

REPO="FurkanCodes/stopcord"
DEST="${STOPCORD_INSTALL_DIR:-/Applications}"

say() { printf '%s\n' "$*"; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

[ "$(uname -s)" = Darwin ] || fail "this installer is for macOS. On Linux, build Stopcord from source."
[ "$(uname -m)" = arm64 ] || fail "the release is built for Apple silicon (M1 or later). On an Intel Mac, build Stopcord from source."

# Signed in to the GitHub CLI, downloads go through it, so this works while the repo is private.
use_gh=""
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  use_gh=1
fi

say "Finding the latest Stopcord release..."
version="${STOPCORD_VERSION:-}"
if [ -z "$version" ]; then
  if [ -n "$use_gh" ]; then
    version="$(gh release view -R "$REPO" --json tagName --jq .tagName 2>/dev/null | sed 's/^v//')" || version=""
  else
    version="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
      | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' | head -n 1)" || version=""
  fi
fi
[ -n "$version" ] || fail "couldn't find the latest release on GitHub. If you were invited to the private repo, install the GitHub CLI, run 'gh auth login', and try again."
name="Stopcord-$version.zip"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

say "Downloading Stopcord $version..."
if [ -n "$use_gh" ]; then
  gh release download "v$version" -R "$REPO" -D "$work" -p "$name" -p "$name.sha256" \
    || fail "couldn't download the release."
else
  url="https://github.com/$REPO/releases/download/v$version"
  curl -fL --progress-bar -o "$work/$name" "$url/$name"
  curl -fsSL -o "$work/$name.sha256" "$url/$name.sha256"
fi

say "Checking the download..."
(cd "$work" && shasum -a 256 -c "$name.sha256" >/dev/null) \
  || fail "the download doesn't match its checksum; nothing was installed."

ditto -x -k "$work/$name" "$work/unpacked"
[ -d "$work/unpacked/Stopcord.app" ] || fail "the zip doesn't contain Stopcord.app."

cat <<'EOF'

A quick heads-up before installing
----------------------------------
Hi! Stopcord is a one-person project, and I don't have a paid Apple Developer
account yet. That means Apple hasn't "notarized" Stopcord, their stamp that
says an app was checked. Once I get the account, I'll notarize Stopcord and
this question goes away.

Until then, macOS blocks apps without that stamp the first time they open.
The app itself is fine: it's open source, and the download was just checked
against the release's checksum. But it's your Mac, so you decide how to
open it:

  YES: skip Apple's check, and Stopcord opens right away.
  NO:  keep Apple's check. macOS blocks Stopcord the first time, and you
       allow it once in System Settings -> Privacy & Security -> Open Anyway.

Either way you get the same app. Pick NO if you'd rather let macOS check it.
EOF

answer=""
if [ -r /dev/tty ] && { exec 3</dev/tty; } 2>/dev/null; then
  while :; do
    printf '\nOK to open Stopcord without Apple'"'"'s check? [yes/no]: ' >/dev/tty
    read -r answer <&3 || answer=""
    case "$(printf '%s' "$answer" | tr '[:upper:]' '[:lower:]')" in
      y|yes) answer=yes; break ;;
      n|no)  answer=no;  break ;;
      "") ;;  # Enter alone, or a key pressed during the download: ask again
      *) say "Please type yes or no." ;;
    esac
  done
  exec 3<&-
else
  say "No terminal to ask in, so keeping the mark (NO)."
  answer=no
fi

app="$DEST/Stopcord.app"
if [ -e "$app" ]; then
  # Only the window quits. The background process that owns your terminals
  # keeps running, so agents at work carry on.
  if osascript -e 'application "Stopcord" is running' 2>/dev/null | grep -q true; then
    say "Quitting the Stopcord window..."
    osascript -e 'quit app "Stopcord"' >/dev/null 2>&1 || true
    sleep 2
  fi
  say "Replacing $app..."
  rm -rf "$app" 2>/dev/null || sudo rm -rf "$app"
fi
mkdir -p "$DEST"
ditto "$work/unpacked/Stopcord.app" "$app" 2>/dev/null || sudo ditto "$work/unpacked/Stopcord.app" "$app"

if [ "$answer" = yes ]; then
  xattr -dr com.apple.quarantine "$app" 2>/dev/null || sudo xattr -dr com.apple.quarantine "$app"
  say ""
  say "Stopcord $version is installed in $DEST. Opening it..."
  [ -n "${STOPCORD_NO_OPEN:-}" ] || open "$app"
else
  mark="0081;$(printf '%x' "$(date +%s)");Stopcord installer;"
  xattr -w com.apple.quarantine "$mark" "$app" 2>/dev/null \
    || sudo xattr -w com.apple.quarantine "$mark" "$app"
  say ""
  say "Stopcord $version is installed in $DEST, with the mark."
  say "Open it from Applications. When macOS blocks it, click Done, then go to"
  say "System Settings -> Privacy & Security and click Open Anyway."
fi
