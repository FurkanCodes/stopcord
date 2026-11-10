#!/usr/bin/env bash
# Build one fixture for the link test: a repo, a factory folder outside it,
# and one worktree wired up the way a case says.
#
#   setup.sh <dir> <case>
#
# Cases:
#   control  the worktree's instruction files are plain copies of the
#            knowledge base (no links), as a baseline
#   link     the worktree's instruction files are symlinks to the factory's
#            knowledge-base.md; tracked ones hidden with skip-worktree
#   second   the repo's own instruction files stay untouched; each harness
#            gets an extra untracked file (or config entry) pointing at the
#            knowledge base, hidden with .git/info/exclude
#
# Code words (each lives in exactly one place, so an answer shows what was read):
#   HERON-41    the factory's knowledge-base.md
#   OTTER-17    the repo's own AGENTS.md / CLAUDE.md (tracked in git)
#   WREN-05     only in the factory's copy, knowledge/repo/AGENTS.md
#   KESTREL-88  the fact the write probe asks the agent to record
set -euo pipefail

dir=${1:?usage: setup.sh <dir> <case>}
case=${2:?usage: setup.sh <dir> <case>}
case $case in control|link|second) ;; *) echo "unknown case: $case" >&2; exit 2 ;; esac

mkdir -p "$dir"
dir=$(cd "$dir" && pwd -P)
repo=$dir/repos/invoice-app
factory=$dir/factories/invoice-app
wt=$dir/worktrees/invoice-app/t1
rm -rf "$repo" "$factory" "$wt"
mkdir -p "$repo" "$factory/knowledge/repo" "$(dirname "$wt")"

# --- the team's repo, with its own instruction files ---------------------
repo_file='# invoice-app

These are the repo'\''s own instructions.

- Code word: OTTER-17
- Run the tests with `make test`.
'
git -C "$repo" init -q -b main
git -C "$repo" config user.name "link test"
git -C "$repo" config user.email "link-test@example.invalid"
printf '%s' "$repo_file" > "$repo/AGENTS.md"
printf '%s' "$repo_file" > "$repo/CLAUDE.md"
printf '# invoice-app\n\nA tiny invoice app.\n' > "$repo/README.md"
mkdir -p "$repo/src"
printf 'export const total = (lines) => lines.reduce((a, l) => a + l, 0);\n' > "$repo/src/totals.js"
git -C "$repo" add -A
git -C "$repo" commit -q -m "Start the invoice app"

# --- the factory folder, outside the repo and the worktrees --------------
# Same wording as the knowledge base example on the architecture page,
# including the relative path in "Before you start".
cat > "$factory/knowledge-base.md" <<EOF
# invoice-app

## About this project
- Code word: HERON-41
- Invoices are built in src/. Totals live in src/totals.js.

## How this factory works
- Its files are in $factory/
- Before you start, read knowledge/repo/AGENTS.md.
  It is this repo's own file.
- When you find something out, add it here.
- Put rules and facts in these files, not in your own memory.
EOF
{ printf '%s' "$repo_file"; printf '\nCopied from main by the factory. Copy marker: WREN-05\n'; } \
  > "$factory/knowledge/repo/AGENTS.md"

# --- one worktree, wired per case ----------------------------------------
git -C "$repo" worktree add -q -b t1 "$wt"
exclude=$(git -C "$wt" rev-parse --path-format=absolute --git-path info/exclude)
mkdir -p "$(dirname "$exclude")"
kb=$factory/knowledge-base.md

case $case in
  control)
    for f in CLAUDE.md AGENTS.md GEMINI.md; do rm -f "$wt/$f"; cp "$kb" "$wt/$f"; done
    git -C "$wt" update-index --skip-worktree CLAUDE.md AGENTS.md
    echo GEMINI.md >> "$exclude"
    ;;
  link)
    for f in CLAUDE.md AGENTS.md GEMINI.md; do rm -f "$wt/$f"; ln -s "$kb" "$wt/$f"; done
    git -C "$wt" update-index --skip-worktree CLAUDE.md AGENTS.md
    echo GEMINI.md >> "$exclude"
    ;;
  second)
    ln -s "$kb" "$wt/CLAUDE.local.md"      # Claude Code: local project instructions
    ln -s "$kb" "$wt/AGENTS.override.md"   # Codex: replaces AGENTS.md for Codex only
    ln -s "$kb" "$wt/SAWTOOTH.md"          # Gemini CLI, via context.fileName below
    mkdir -p "$wt/.gemini"
    printf '{\n  "context": { "fileName": ["AGENTS.md", "SAWTOOTH.md"] }\n}\n' > "$wt/.gemini/settings.json"
    printf '{\n  "$schema": "https://opencode.ai/config.json",\n  "instructions": ["%s"]\n}\n' "$kb" > "$wt/opencode.json"
    printf '%s\n' CLAUDE.local.md AGENTS.override.md SAWTOOTH.md .gemini/ opencode.json >> "$exclude"
    ;;
esac

# The worktree must look clean to git, or the design fails before any harness runs.
if [ -n "$(git -C "$wt" status --porcelain)" ]; then
  echo "setup: worktree is not clean:" >&2
  git -C "$wt" status --porcelain >&2
  exit 1
fi

printf 'repo=%s\nfactory=%s\nwt=%s\n' "$repo" "$factory" "$wt"
