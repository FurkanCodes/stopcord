# Link test

Checks the plan on the architecture page: each worktree's instruction files
(CLAUDE.md, AGENTS.md, GEMINI.md) are links to the factory's
`knowledge-base.md`, the repo's own file is kept in `knowledge/repo/`, and
the knowledge base tells agents to read it before they start.

It answers four questions per harness:

1. **Load.** Does the harness read an instruction file that is a symlink to
   a file outside the worktree?
2. **Follow.** Told "Before you start, read knowledge/repo/AGENTS.md" (a
   relative path, the page's exact wording), does the agent find and open
   the factory's copy from inside a worktree?
3. **Write.** Asked to record a fact, does it land in the factory's
   knowledge base, and does the link survive the edit?
4. **Git.** What does a hidden link (skip-worktree) do when main changes
   the same file? (`git-check.sh`, no harness needed.)

## Cases

| case | worktree's instruction files | what it tells us |
|---|---|---|
| `control` | plain copies of the knowledge base | the baseline: does the harness load its file at all in this mode |
| `link` | symlinks to the factory's `knowledge-base.md`, tracked ones hidden with `skip-worktree` | the plan as written |
| `second` | the repo's own files untouched, plus an untracked extra pointing at the knowledge base (`CLAUDE.local.md`, `AGENTS.override.md`, `STOPCORD.md` via `.gemini/settings.json`, `opencode.json` `instructions`), hidden with `.git/info/exclude` | the alternative that never touches a tracked file |

Each file holds one code word, so an answer shows exactly what was read:
`HERON-41` the knowledge base, `OTTER-17` the repo's own file, `WREN-05`
only the factory's copy in `knowledge/repo/`, `KESTREL-88` the fact the
write probe records.

## Run it

```sh
./git-check.sh                      # git only, takes a second
./run.sh --dry-run                  # print every harness command, run nothing
./run.sh                            # everything installed: 4 harnesses x 3 cases x 3 probes
./run.sh --harness codex --case link
```

Fixtures and raw output go to `$TMPDIR/stopcord-link-test/<time>/`. The
summary table is copied to `results/<time>.md`. Each run is one short
prompt, but a full run is 36 of them on your own accounts and models.

## Things to know

- Gemini CLI has to be logged in first (`gemini` once, interactively),
  or every Gemini run stops at the browser sign-in.
- Harnesses run with your own settings, global instructions and hooks, the
  same way Stopcord would launch them. Global instruction files don't
  contain the code words, so they don't affect the result.
- Non-interactive modes deny what an interactive session would ask about.
  A "denied" in the summary means the person would have seen a permission
  prompt there.
- The load probe asks the agent not to use tools. Claude Code gets no tools
  at all; for the others the summary counts tool calls, and a load answer
  with tool calls doesn't prove anything about loading.

## Results so far

`git-check.sh` (2026-11-08): with `CLAUDE.md` linked and hidden by
`skip-worktree`, once main changes `CLAUDE.md` both `git rebase main` and
`git merge main` stop with "Your local changes to the following files would
be overwritten". `git add -A` does leave the link out, as intended.

Smoke run (control case, load probe): Claude Code, Codex and opencode each
loaded the knowledge base. Gemini was not logged in.
