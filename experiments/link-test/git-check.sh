#!/usr/bin/env bash
# Git-only check, no harness needed: what happens to a worktree whose tracked
# CLAUDE.md is a hidden link (skip-worktree) when main changes that file and
# the worktree takes in main (rebase, merge, or a fresh checkout)?
#
#   git-check.sh [dir]
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
dir=${1:-${TMPDIR:-/tmp}/stopcord-link-test/git-check}
rm -rf "$dir"
eval "$("$here/setup.sh" "$dir" link)" || exit 1

# The team edits its own CLAUDE.md on main.
printf -- '- New team rule: use pnpm.\n' >> "$repo/CLAUDE.md"
git -C "$repo" commit -q -am "Team: add a rule to CLAUDE.md"

# The agent commits some work on its branch.
printf 'export const tax = 0;\n' > "$wt/src/tax.js"
git -C "$wt" add src/tax.js
git -C "$wt" commit -q -m "Add tax"

report() {
  local name=$1; shift
  local out rc
  out=$("$@" 2>&1); rc=$?
  printf '\n## %s  (exit %s)\n%s\n' "$name" "$rc" "$out"
  printf 'CLAUDE.md now: %s\n' "$(if [ -L "$wt/CLAUDE.md" ]; then echo "link -> $(readlink "$wt/CLAUDE.md")"; else echo "plain file"; fi)"
  printf 'git status: %s\n' "$(git -C "$wt" status --porcelain | tr '\n' ' ')"
}

report "rebase onto main" git -C "$wt" rebase main
git -C "$wt" rebase --abort >/dev/null 2>&1
report "merge main" git -C "$wt" merge --no-edit main
git -C "$wt" merge --abort >/dev/null 2>&1
report "add -A then commit (an agent tidying up)" sh -c "git -C '$wt' add -A && git -C '$wt' commit -q -m tidy; git -C '$wt' log --stat -1 --format=%s"
