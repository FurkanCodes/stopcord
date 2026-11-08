#!/usr/bin/env bash
# Run each installed harness against each case with three probes, then
# summarize. Every (harness, case) gets its own fresh fixture, because the
# write probe changes the factory folder.
#
#   run.sh [--harness claude,codex,gemini,opencode] [--case control,link,second]
#          [--probe load,follow,write] [--root DIR] [--dry-run]
#
# Probes, run in this order in the same worktree:
#   load    tools off (or asked not to use them): which code words did the
#           harness put in front of the agent at startup?
#   follow  read-only tools: does the agent do what the knowledge base says
#           and open the repo's own file before it starts?
#   write   edits allowed: asked to record a fact, where does it land, and
#           does the link survive?
set -uo pipefail

here=$(cd "$(dirname "$0")" && pwd)
harnesses=claude,codex,gemini,opencode
cases=control,link,second
probes=load,follow,write
root=${TMPDIR:-/tmp}/sawtooth-link-test
dry=0
timeout_s=${LINK_TEST_TIMEOUT:-300}

while [ $# -gt 0 ]; do
  case $1 in
    --harness) harnesses=$2; shift 2 ;;
    --case) cases=$2; shift 2 ;;
    --probe) probes=$2; shift 2 ;;
    --root) root=$2; shift 2 ;;
    --dry-run) dry=1; shift ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
done

P_LOAD='Answer from memory only. Do not read any files and do not run any tools or commands. Your instructions may contain code words shaped like WORD-NN, for example ABC-12. Reply with every such code word you were given, one per line, and nothing else. If you were given none, reply NONE.'
P_FOLLOW='Get ready to work in this repo the way your instructions say. Do not change any files. Then reply with every code word shaped like WORD-NN, for example ABC-12, that you have seen so far, one per line, and nothing else. If you have seen none, reply NONE.'
P_WRITE='Record this fact the way your instructions say facts should be recorded: "The test fact code is KESTREL-88." Then reply DONE.'

# Run a command with a time limit, stdin closed, output to a file.
limited() { local out=$1; shift; perl -e 'alarm shift; exec @ARGV or die "exec: $!"' "$timeout_s" "$@" </dev/null >"$out" 2>"$out.err"; echo $? >"$out.exit"; }

harness_cmd() { # harness probe worktree prompt outfile -> prints nothing, runs
  local h=$1 probe=$2 wt=$3 prompt=$4 out=$5
  case $h in
    claude)
      local tools=""
      case $probe in follow) tools="Read,Glob,Grep,Bash" ;; write) tools="Read,Glob,Grep,Bash,Edit,Write" ;; esac
      local extra=(); [ "$probe" = write ] && extra=(--permission-mode acceptEdits)
      (cd "$wt" && run limited "$out" claude -p "$prompt" --output-format stream-json --verbose ${extra[@]+"${extra[@]}"} --tools "$tools") ;;
    codex)
      local sb=read-only; [ "$probe" = write ] && sb=workspace-write
      run limited "$out" codex exec --json --ephemeral --sandbox "$sb" -C "$wt" -o "$out.last" "$prompt" ;;
    gemini)
      local mode=default; [ "$probe" = write ] && mode=auto_edit
      (cd "$wt" && run limited "$out" gemini -p "$prompt" -o json --approval-mode "$mode") ;;
    opencode)
      run limited "$out" opencode run --format json --dir "$wt" "$prompt" ;;
  esac
}

run() { if [ $dry = 1 ]; then printf '  $'; printf ' %q' "${@:3}"; printf '\n'; else "$@"; fi; }

stamp=$(date +%Y%m%d-%H%M%S)
out_root=$root/$stamp
mkdir -p "$out_root"
echo "results: $out_root"

IFS=, read -ra H <<<"$harnesses"; IFS=, read -ra C <<<"$cases"; IFS=, read -ra PR <<<"$probes"
for h in "${H[@]}"; do
  if ! command -v "$h" >/dev/null; then echo "skip $h: not installed"; continue; fi
  for c in "${C[@]}"; do
    d=$out_root/$h/$c
    eval "$("$here/setup.sh" "$d" "$c")" || { echo "setup failed for $h/$c" >&2; continue; }
    for p in "${PR[@]}"; do
      case $p in load) prompt=$P_LOAD ;; follow) prompt=$P_FOLLOW ;; write) prompt=$P_WRITE ;; *) echo "unknown probe $p" >&2; exit 2 ;; esac
      echo "$h / $c / $p"
      harness_cmd "$h" "$p" "$wt" "$prompt" "$d/$p.raw"
    done
    # What the write probe left behind.
    {
      for f in CLAUDE.md AGENTS.md GEMINI.md CLAUDE.local.md AGENTS.override.md SAWTOOTH.md; do
        if [ -L "$wt/$f" ]; then echo "$f link"; elif [ -e "$wt/$f" ]; then echo "$f file"; fi
      done
      echo "status $(git -C "$wt" status --porcelain | tr '\n' ';')"
      grep -q KESTREL-88 "$factory/knowledge-base.md" && echo "fact factory-kb"
      grep -rlq KESTREL-88 "$factory" --exclude=knowledge-base.md 2>/dev/null && echo "fact factory-other $(grep -rl KESTREL-88 "$factory" --exclude=knowledge-base.md | tr '\n' ' ')"
      for f in $(grep -rl KESTREL-88 "$wt" --exclude-dir=.git 2>/dev/null); do [ -L "$f" ] || echo "fact worktree ${f#$wt/}"; done
      echo "factory $factory"
      echo "wt $wt"
    } >"$d/state.txt"
  done
done

[ $dry = 1 ] && exit 0
python3 "$here/summarize.py" "$out_root" | tee "$out_root/summary.md"
mkdir -p "$here/results"
cp "$out_root/summary.md" "$here/results/$stamp.md"
echo
echo "summary saved to $here/results/$stamp.md"
