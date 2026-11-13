# First ticket: New factory defaults, a first test run, and stage notes

*   **Author:** Furkan
*   **Status:** Approved (revised after the Goldfish review, stages 5–9)
*   **Date:** 2026-11-12

--------------------------------------------------------------------------------

## Overview

Sawtooth runs a factory for each project. A ticket goes in. The engine starts a
planner, a builder and a reviewer in turn. A person approves the plan and
decides at handoff whether to merge. This design makes the first pass through
that loop work for someone who has never used Sawtooth. They open the app with
no factories, pick a project folder, write one small ticket, approve its plan,
and decide at handoff, without reading anything outside the app.

The change is to three screens that already exist (Home's empty state, New
factory, and Work), plus three engine fixes that make what those screens say
true. Every change applies to every user, every time.

### Glossary

*   **Line:** the sequence of stages a ticket goes through: plan, approve,
    build, verify, review, handoff.
*   **Quick checks:** what New factory checks while you pick a folder: git repo,
    has a commit, each harness installed. No command from the repo runs.
*   **Test run:** one run of the factory's test command on a copy of the
    project's last commit, after create or when asked. It predicts whether the
    verify stage and the commit hook will pass.
*   **Gate checks:** the checks the verify stage runs on each ticket's branch.
    Unchanged by this design.
*   **Safety statement:** the lines New factory shows before create that say
    what create writes and what agents can and can't change.
*   **Setup command:** a runner's `setup_commands`, run in each new worktree
    before its agent starts (`app/src-tauri/src/runner.rs:385-394`).

## The Problem

A new user meets one paragraph and a "+ new factory" button. From there:

*   **New factory asks for about ten decisions before anything happens**
    (`app/src/NewFactory.tsx:235-433`): folder, harness, reviewer harness,
    runner level, name, and more. It uses words the app never defines: line,
    harness, runner, contained, gate, handoff.
*   **After create, the user lands in Work with six tabs and a canvas,** and has
    to know to press `w` to write a ticket.
*   **The app says things that aren't true for their setup:**
    *   "Nothing reaches your branch until you approve" fails for folders that
        aren't git repos: agents work in the folder itself
        (`app/src-tauri/src/worktree.rs:44-45`). At the `worktree` level it
        rests on a git hook that isn't installed when the repo uses
        `core.hooksPath` (`app/src-tauri/src/hooks.rs:76-84,171-179`) or the
        project folder has no branch checked out (`hooks.rs:136`). In both
        cases the app says nothing about the missing hook.
    *   Create edits the repo without saying so: a line in an existing CLAUDE.md
        or AGENTS.md (`app/src-tauri/src/kb.rs:164-174`), and commit and push
        hooks that run the factory's checks on every commit in the repo,
        including the person's own (`hooks.rs:355-419`). New factory mentions
        the hooks only for the reviewed template (`NewFactory.tsx:188`), though
        a line installs the same ones.
    *   "It will tell you when it needs you" has no way to reach anyone outside
        the window: no OS notifications, no dock badge.
*   **The first ticket fails for reasons the person didn't cause:**
    *   **Missing dependencies.** A new worktree has no `node_modules`, and the
        default runner has no setup command (`app/src-tauri/src/templates.rs:31-36`).
        In a JS repo, the builder's first commit runs the tests through the
        commit hook, and they fail before verify is reached.
    *   **Tests that already fail.** The test command is guessed from file
        names and never run (`app/src-tauri/src/project.rs:52-84`). If tests
        already fail on the base branch, every commit is stopped by the commit
        hook (`hooks.rs:410-419`), and verify sends the ticket back to build up
        to three times.
    *   **A harness that isn't installed.** "Installed" is checked, but nothing
        stops create when the reviewer's harness is missing. Today the reviewer
        defaults to the second installed harness, which the person may never
        have signed in to (`NewFactory.tsx:37`).
*   **The first ticket usually stops somewhere nobody explains:** an empty plan
    for a small ticket, a protected-test finding that says "the gate ran the
    base branch's copy" for a test the branch added and the gate ran as written
    (`app/src-tauri/src/flow.rs:31`), review sending work back, a merge
    conflict. Each shows a short status word, not what happened or what to do.
*   **Problems found while setting up a factory show in only one place.**
    `create_factory` returns them in `FactoryInfo.problems`
    (`app/src-tauri/src/main.rs:384-391`), but only the map's side pane shows
    them.

The person who built the app reports not knowing how to use it. The first real
use (commits `3961f7e`, `1a282b7`) turned up the line failing silently or
confusingly. Both point the same way: the hard part isn't learning the ideas.
It's that the first run stops in ways the app doesn't explain, after promises
it can't keep.

## Goals

1.  **A first ticket with nothing to look up.** A person with a coding-agent CLI
    installed and signed in, and a git repo with at least one commit, can go
    from an empty Home to a first ticket at handoff using only what the app
    shows them.
2.  **One decision in New factory: the folder.** Everything else has a default,
    shown on one summary line that unfolds to change it.
3.  **Every statement before create is true for this repo.** The safety
    statement is built from what New factory detected: git or not, the branch
    checked out, the runner level, which files create writes, which checks the
    commit hook will run, and whether create runs the tests. Problems that can
    only be found by creating (a hook that couldn't be installed) show right
    after create.
4.  **Problems that can be found before a ticket starts are found before it
    starts.** Quick checks: git repo, has a commit, each harness on the line
    installed. The test run: passed, failed (with what to do), or couldn't tell
    (with the reason).
5.  **Every state a ticket can stop in says what happened and what each action
    does,** in one or two plain sentences. The states are listed in the stage
    notes table.
6.  **Problems found while setting up a factory show where the person is
    looking:** in Work right after create, not only in the map's side pane.
7.  **The test run stays useful after the first ticket.** Tests break later
    too, so it can be run again from Setup.

## Non-Goals

*   **An onboarding mode, tour, wizard or separate route.** No flag switches
    guidance on or off.
*   **Teaching features beyond the first ticket.** A separate docs website will
    cover triggers, races, panels, scorers, knowledge, sources and the rest.
*   **A sign-in check for harnesses.** Deferred (D3).
*   **Setup commands for languages other than JavaScript.** Cargo, Go and
    Python fetch their own dependencies when tests run, or need a choice of
    tool the app can't guess. The Setup screen's existing field covers them.
*   **A bundled sample project, or a practice copy of the user's repo**
    (concept question 9). Decide after watching one real new user.
*   **Measuring onboarding,** locally or remotely (concept question 11).
*   **A way for e2e tests to skip onboarding** (concept question 12). There is
    no onboarding mode to skip. The test run is switched off for e2e (D2).
*   **Home's handling of factories that fail to load.** It's a bug for existing
    users and ships as its own change.
*   **General UI polish** outside the three screens.
*   **Code that branches on harness names** (PRODUCT.md: "No screen assumes
    one").
*   **Celebration or chirpy copy.** The brand stays calm, exact and literal.

## Critical User Journeys (CUJs)

### CUJ 1: First ticket, nothing goes wrong

*   **Given**: Sawtooth has no factories. Claude Code is installed and signed
    in. The machine can sandbox. The user has a JS repo with commits, a
    `package-lock.json` and tests that pass.
*   **When**: They open the app, choose "+ new factory", and pick the repo
    folder.
*   **Then, in New factory**:
    *   The quick checks show "git repo", "has a commit" and "claude found" as
        passed.
    *   The summary line reads "a line · claude · contained · setup npm ci ·
        links AGENTS.md, CLAUDE.md".
    *   The safety statement says what create writes, that the commit hook runs
        `npm test` and "no secrets" on every commit, theirs too, that agents
        can't move the branch checked out now, and that create runs `npm ci`
        then `npm test` once in the sandbox.
*   **Then, after create**: Work opens with the test run showing "running…" and
    "press w to write a ticket". They can write the ticket while the test run
    goes on. The test run shows "passed" a few minutes later.
*   **Then, on the ticket**: after they approve the plan, the ticket detail says
    which stage is running and what comes next. At handoff it says what merging
    does and which branch it goes to.

### CUJ 2: The tests already fail

*   **Given**: Same as CUJ 1, but the repo's tests already fail on its main
    branch.
*   **When**: They create the factory.
*   **Then**: The test run shows "failed" with the last lines of output, and
    says that while these tests fail, every commit in the repo is stopped,
    theirs and the agents'. It says to make them pass, or to change when the
    tests part runs, in Setup. Writing a ticket isn't blocked, but the failure
    stays shown next to "press w to write a ticket" until the test run passes.
    No agent has run yet.

### CUJ 3: The first ticket stops in a way that needs the person

*   **Given**: A factory exists and a first ticket is running.
*   **When**: The ticket stops: the planner wrote no plan, or the builder added a
    test file the gate protects, or the builder's harness stopped without
    finishing.
*   **Then**: The ticket detail says in one or two sentences what happened and
    what each available action does. Home's "next up" lists it. The approve and
    handoff notes say that Sawtooth doesn't notify anyone outside the app (D4).

### CUJ 4: An existing user sees the same screens

*   **Given**: A user with five factories and weeks of use.
*   **When**: They create a sixth factory and run tickets as usual.
*   **Then**: They see the same quick checks, safety statement, test run and
    stage notes as a new user. They press `c` to unfold New factory's options.
    Nothing appears only once, nothing asks to be dismissed, and their existing
    factories don't change, with two exceptions: a protected test a branch added
    gets the corrected finding text (D5), and a factory whose branch guard
    couldn't be installed now says so in its problems (D1).

## Technical Plan (Architecture)

### Shape of the change

```text
 Home (no factories)        New factory                          Work (factory, no tickets yet)
 ───────────────────        ───────────                          ──────────────────────────────
 what a factory does  ──▶   1. pick folder                  ──▶  4. test run (when create started it,
 + new factory              2. quick checks: git repo,              or "run tests"): setup command,
                               has a commit, harness found          then test command, on a copy of
                            3. safety statement under the           the last commit
                               existing "will create" tree       5. problems create found
                            [create]                             6. press w to write a ticket
                            options folded behind one line       7. ticket detail: one note per state,
                                                                    agent → human → agent → gate → agent → human

 Setup › test run: "run tests" runs step 4 again at any time.
```

Nothing is keyed on "first run". Work's empty state is what Work shows for a
factory with no work items. Once a ticket exists, the test run stays in Setup.

### Decisions

The concept review's questions map to decisions as follows: question 2 → D1,
3 → D2, 4 → D3, 5 → D4, 6 → D5, 7 → D6, 8 → D7, 10 → D8. Questions 9, 11 and 12
are Non-Goals.

#### D1. The safety statement

*   **A line needs a git repo with at least one commit.** Without one, the quick
    check fails and create is disabled for a line, with the rule and its fix
    (copy R-git, F-no-git, F-no-commit). The open and reviewed templates still
    work on any folder.
*   **The runner level defaults to `contained` when the machine can sandbox, and
    to `worktree` otherwise.** The window and `create_factory`'s fallback use
    the same rule. Today the window defaults to `contained` and the backend to
    `worktree`.
*   **The statement is the existing "will create" tree** (`NewFactory.tsx:170-189`)
    plus three lines under it:
    *   the hooks line, for the line template as well as reviewed (S-hooks);
    *   one sentence for the runner level, naming the branch checked out now
        (S-contained, S-worktree, S-vm, S-harness), or S-no-branch when no
        branch is checked out;
    *   whether create runs the tests (D2).
*   **A hook that can't be installed shows after create, not before.** Its
    cause (`core.hooksPath`, an existing hook of that name, no branch) can only
    be found by installing. These arrive as `FactoryInfo.problems`, and Work
    shows them right after create. Two cases that say nothing today get a
    problem: no branch checked out (P-no-branch), and `core.hooksPath` when the
    guard was due (P-guard-hookspath). Because `apply` runs for every factory
    at start-up (`main.rs:1112`), existing factories show these too.

#### D2. Dependencies and the test run

*   **The default runner gets a setup command from the lockfile.** When the
    folder has a `package.json`, the lockfile picks the command:

    | Lockfile | Setup command | Sites added to `network_allow` |
    | --- | --- | --- |
    | `pnpm-lock.yaml` | `pnpm install --frozen-lockfile` | `registry.npmjs.org` |
    | `yarn.lock` | `yarn install --frozen-lockfile` | `registry.yarnpkg.com`, `registry.npmjs.org` |
    | `bun.lock` or `bun.lockb` | `bun install --frozen-lockfile` | `registry.npmjs.org` |
    | `package-lock.json` | `npm ci` | `registry.npmjs.org` |

    The order matches `project::test_command`'s, so the setup and test commands
    use the same tool. With no lockfile, no setup command is added. The sites
    are added at every level. Only `contained` filters the web, so at other
    levels they have no effect, and the runner keeps working if the level is
    changed later.
*   **The setup command already runs where it's needed.** It runs in each new
    worktree before its agent starts, under the agent's own confinement, once
    per worktree (`runner.rs:385-394`). Isolated copies carry the "already set
    up" marker (`runner.rs:376-382`). Because the dependencies are in the
    worktree, the commit hook finds them, and the gate's snapshot copies them
    in with the other ignored files (`gate.rs:54-77,127`).
*   **The test run:**
    1.  It makes a copy of the project's last commit (`HEAD`), using the
        gate's own worktree code, in `state/gates/test-run`. Unlike the gate's
        snapshot, it ignores uncommitted changes, because create itself leaves
        one (the line added to CLAUDE.md). It copies no ignored files, so it
        starts from what a new worktree has.
    2.  It runs the default runner's setup commands in the copy, with the
        network open.
    3.  It runs the tests part's command, with the part's own network setting.
    4.  Both steps use the gate's way of running a check: `confined_check`,
        then `run_command` with `GATE_TIMEOUT` (30 minutes). This code is
        shared with the verify gate and benchmarks (step 4 of the plan).
*   **The result is a run in the factory's runs log,** not a new store. A
    run's `ok` already holds passed or failed. The test run's records are the
    runs with `kind: "check"`, `part: "tests"`, `by: "engine"` and no `work`.
    No other run has that shape: gate runs have `work`, and hook runs have no
    `by`.
    *   **Started:** `ok` and `detail` both absent.
    *   **Passed:** `ok: true`.
    *   **Failed:** `ok: false`, the explanation in `text`, the output's last
        40 lines in `detail`.
    *   **Couldn't tell:** `ok` absent, the reason in `detail`. This covers a
        copy that couldn't be made, a check that couldn't start, the 30-minute
        limit, and the app closing mid-run. `detail` is never empty here:
        `Run::detail` drops empty text (`runs.rs:83-88`), so an empty error
        becomes "no error text", and T-cant-closed's `detail` is "The app
        stopped during the run." Otherwise a cut-off run would read as
        started.

    The newest such run is the current result. A busy factory can log more than
    the 200 runs the window loads (`app/src/store.ts:223`), so the window
    doesn't search its own list. A new command, `test_run(id)`, scans the whole
    log for the newest test-run record. The window calls it again on each
    `factory:runs` event (`store.ts:290`), so no new event is needed. A factory's
    log is small text, and the scan only runs while a test run is on screen.
*   **When it runs:**
    *   **Once after create**, when all of these hold: the template is `line`
        (not reviewed or a saved template, which New factory doesn't describe
        a test run for); the factory has a tests part; the machine can
        sandbox; the folder wasn't cloned in this New factory, by either
        clone path; and `SAWTOOTH_NO_AUTO_TESTS` isn't set.
    *   **Otherwise, only when asked:** "run tests" in Work's empty state or in
        Setup.

    Without a sandbox, the repo's code would run with the person's full rights.
    A folder that was just cloned may hold code nobody has read. In both cases
    the person decides. The e2e harness sets `SAWTOOTH_NO_AUTO_TESTS`, so e2e
    tests never start a test run they didn't ask for.
*   **Failed tests stop every commit,** because a line's tests part runs on
    commit (`main.rs:338-346`) and the hooks are shared by every worktree. The
    failed text says so and gives the two fixes that remove the block: make the
    tests pass, or change when the tests part runs, in Setup (T-failed).
    Taking verify out of the line wouldn't help, because the hook still runs.
*   **One test run at a time per factory.** A second "run tests" while one is
    running is refused, with E-running.
*   **No test command:** a line factory shows T-none in place of the test run.
    The existing note in New factory (`NewFactory.tsx:404-406`) moves out of the
    folded options into the quick checks.

#### D3. Sign-in check: deferred

The reviewer now defaults to the builder's harness, so a line usually has one
harness. A signed-out harness then shows up at plan, the first stage, before
any other stage has spent anything. The failed-agent note (N-agent-failed)
points at the terminal, where the harness shows its own login prompt. A sign-in
check would cost a model call each run, and telling a sign-in error apart from
other errors means reading harness-specific output. It can be added later for a
reviewer harness that differs from the builder's.

#### D4. "It tells you when it needs you"

The promise is reworded, not built. The approve and handoff notes end with D4-say.
OS notifications are under Future Improvements.

#### D5. Protected tests: correct the finding's text

What counts as a changed protected test doesn't change. A test file the branch
added still raises a finding, and the existing "accept its test changes" action
(from `3961f7e`) still handles it. Only the text changes: for an added file,
the gate ran the branch's own version, because there was no base copy to put
back (`gate.rs:128-135`). Added files get F-protected-added; changed and deleted
files keep the existing text. Accepting tests clears both kinds: today it only
rewrites findings that start with the old text (`flow.rs:1041`).

#### D6. Empty plans

The planner's prompt is unchanged. When the plan stage ends without writing a
plan, the approve note says N-approve-empty.

#### D7. Notes and "done"

There is no first-run concept to finish. Every note shows every time, with no
"seen" state. A note is chosen by the ticket's state (stage, waiting value, the
current stage's kind, loop counts and findings), never by stage id, because ids
can be renamed and verify may be missing.

#### D8. Cost

New factory's "a line" option and Work's empty state both say what each ticket
runs (D8-say). There is no time estimate, because nothing has measured one.

### Defaults in New factory

*   **The folder is the only thing shown at first.** Everything else is folded
    under one summary line, which `c` or its "change" button unfolds.
*   **What the fold holds:** name, template (including saved templates), harness,
    reviewer, runner, and link-as.
*   **What stays outside it:** the folder field and picker, clone, the quick
    checks, the "will create" tree with the safety statement, and create.
*   **The folded fields stay mounted and are hidden** with the `hidden`
    attribute, so their element ids and values stay readable. The wrapper
    carries no inline `display` style, which would override `hidden`. The fold
    stays open once unfolded, for the rest of the screen's life.
*   **The reviewer defaults to the builder's harness,** not the second installed
    one. A second harness is something a person chooses.
*   **The runner level** is `levelTyped ?? (sandbox === null ? 'contained' :
    'worktree')`. `sandbox` comes from New factory's own call to
    `api.machine()` (`Machine.sandbox`: null when the machine can sandbox,
    otherwise the reason; `undefined` until it answers). While it's
    `undefined`, the summary leaves the level out, the sandbox fact row isn't
    shown, and create is disabled with `whyNot` "checking this machine's
    sandbox…". The call takes well under a second, and this keeps a fast click
    from creating with the wrong level.

### Quick checks

They run as the folder's `detectProject` result and `installed` arrive. Each row
has a result word, so a result never depends on colour alone.

| Row | Shown when | Passed when | Failed text |
| --- | --- | --- | --- |
| git repo | a line or a saved template with a line | `info.git` | F-no-git |
| has a commit | same, and `info.git` | `info.commits` | F-no-commit |
| `<id>` found | always | `installed` has `program(harness)` | F-harness |
| `<id>` found (reviewer) | `harnessName(profiles, reviewer) !== harnessName(profiles, harness)` | `installed` has `program(reviewer)` | F-harness |

`installed` holds program names, so the found check compares the command's
program (`program`, which `app/src/harness.ts` gains an `export` for): `claude
--model opus` becomes `claude`, and `kilo` stays `kilo` though its profile id is
`kilocode`. The label's `<id>` and the "differs" test use the profile id from
`harnessName(profiles, command)` (`harness.ts:30`). Two fact rows follow
the checks, without a result word: S-no-sandbox when `sandbox` isn't null, and
T-none-nf when a line has no test command.

A missing harness doesn't disable create; it fails at the first stage that uses
it, and F-harness says so.

### Create rules

*   **Window:** `whyNot` gains R-git when `lineChosen && info !== null &&
    !willClone && (!info.git || !info.commits)`. It comes right after the
    existing "pick a folder" check and before the reviewer check. `lineChosen` is `template ===
    'line'`, or a saved template whose `line` isn't empty. While `info` is null
    (still detecting, a detect error, or a clone), the rule doesn't apply.
*   **Backend:** `create_factory` refuses the same case with `Failure::new("git",
    R-git + " " + fix)`, where the fix is F-no-git or F-no-commit. The test is
    `from.map_or(template == "line", |t| !t.line.is_empty())`, and a folder
    with a commit. A cloned repo always has a commit, so cloning still works.

### Stage notes in Work

Work's ticket detail picks a `say` line and actions per state
(`app/src/factory/Work.tsx:603-643`). This design replaces the `say` text with
`noteFor`, a pure function. The actions don't change.

```text
noteFor({ item, w, stage, factory, terminals, evidence, findings, highs,
          testChanges, uncommittedBlock }) -> string
```

*   `stage` is the current `LineStage` (looked up from `item.lineStage`).
*   `highs`, `testChanges` and `uncommittedBlock` are today's `highs`,
    `testChanges` and `blocked` (`Work.tsx:555`, renamed in this function
    only, because it means uncommitted changes at handoff, not a blocked site).
*   Blocked sites keep their own rows (`SiteRequest`, `Work.tsx:680-682`) and
    get no note.

`<next>` is the id of the stage after the current one (`stage.then`). Rules are
checked in order, and the first match wins:

| # | Rule | Note |
| --- | --- | --- |
| 1 | the ticket isn't open, or `item.merging` is set | no note; the existing closed and merging lines stay (`Work.tsx:832-845`) |
| 1a | `w` is `approve` or `handoff` and `evidence` is still undefined (loading) | no note until it arrives |
| 2 | `w === 'approve'` and the plan is empty | N-approve-empty |
| 3 | `w === 'approve'` | N-approve-plan |
| 4 | `w === 'question'` | today's text, unchanged |
| 5 | `w === 'failed'`, gate stage, `testChanges` not empty | N-accept-tests |
| 6 | `w === 'failed'`, gate stage | N-gate-failed |
| 7 | `w === 'failed'`, agent stage | N-agent-failed |
| 8 | `w === 'handoff'`, `testChanges` not empty | N-accept-tests |
| 9 | `w === 'handoff'`, `item.mergeError` set | no note; the merge error already shows, with its own "send it back" advice (`Work.tsx:847-849`, `flow.rs:1181`) |
| 10 | `w === 'handoff'`, `uncommittedBlock` | today's empty text, unchanged |
| 11 | `w === 'handoff'`, `highs > 0` | N-handoff-findings |
| 12 | `w === 'handoff'` | N-handoff |
| 13 | `w === 'landing'` | today's text, unchanged |
| 14 | `item.stage === 'queued'` | N-queued |
| 15 | running, and either a later stage `s` with `s.onFail === stage.id` has `item.loops[s.id] > 0`, or some `item.loops['send:…']` is above 0 | N-sent-back |
| 16 | running, gate stage | N-gate-running |
| 17 | running, agent stage | N-agent-running |
| 18 | anything else | today's `statusOf(item, terminals)` |

**The empty plan** is when no `evidence.files` entry has `path === 'PLAN.md'`,
and no `evidence.uncommitted` line names it. Those lines are `git status
--porcelain` lines such as `?? PLAN.md`, so the test is the text after the
three-character status. `evidence.plan` isn't used for this, because it reads the
worktree's `PLAN.md` (`app/src-tauri/src/git.rs:134`), which can be the repo's
own older file.

**Rule 15** uses the loop counts the line keeps
(`app/src-tauri/src/line.rs:182,337,424`). They cover review sending work back,
verify failing back to build, and a person's send-back (`send:<stage>`). The
counts never reset, so after a stage has once been sent back, every later run
of it gets this note. That's why N-sent-back names neither the sender nor a
count: both can be stale.

Notes are plain text in the existing status area, with no colour or motion, in
line with PRODUCT.md.

### Work's empty state

When the factory has no work items and `canWrite` is true (it has a line), the
right-hand column shows these, in place of the empty `<span />` it renders when
no item is picked (`Work.tsx:254`). The left "work" pane keeps its existing
empty text (`Work.tsx:214-224`). Top to bottom:

1.  For a line factory: the test run (`TestRun` component), or T-none when there
    is no tests part.
2.  One row per entry in `factory.problems`, with the result word "problem".
3.  D8-say.

CUJ 1's "press w to write a ticket" is the left pane's existing empty text.

### Copy

Every string the design adds or changes, written once. Placeholders: `<branch>`
is `info.branch` in New factory and the project's current branch in the engine
(`HEAD` when none is checked out; in New factory S-no-branch then replaces the
level sentence, and S-tests-auto says "the last commit"); `<id>` is a harness
id; `<next>` is the next stage's id; `<target>` is `evidence.target`, or "the
branch checked out in the project folder" when that's empty; `<check names>` is
the names of the parts in the stage's `checks`; `<n>` and `<at_once>` are
numbers. Lowercase is the app's style for UI copy.

**New factory**

| Id | Text |
| --- | --- |
| R-git | a line needs a git repo with at least one commit. |
| F-no-git | run git init in this folder, then make a first commit. |
| F-no-commit | make a first commit in this folder, then pick it again. |
| F-harness | `<id>` isn't installed on this machine, so the stages that use it will stop. install it, or pick another harness under change. |
| S-no-sandbox | no sandbox on this machine · agents run in a worktree instead |
| T-none-nf | no test command found, so this line has no verify stage. add one later in setup |
| Summary, line | `a line · <harness id> · <level> · setup <setup command> · links <links>`, with `reviewer <id>` after the harness when it differs, and the setup part left out when there's none |
| Summary, reviewed | `reviewed · links <links>` |
| Summary, open | `open · links <links>` |
| Summary, saved | `<template name> · links <links>` |
| Summary, no links | `links` part reads `links nothing` |
| S-hooks | git hooks run these on every commit in this repo, yours too: `<check names>` |
| S-contained | agents work in their own worktree and branch, inside the sandbox, which keeps `<branch>` (checked out now) read-only to them. |
| S-worktree | agents work in their own worktree and branch, with your rights. a git hook stops them moving `<branch>` (checked out now). |
| S-vm | agents work in a linux virtual machine, in their own worktree and branch. |
| S-harness | agents work in their own worktree and branch, inside their harness's own sandbox. |
| S-no-branch | no branch is checked out in this folder, so nothing stops agents moving one. check out a branch first. |
| S-tests-auto | after create, the tests run once in the sandbox on a copy of `<branch>`'s last commit: `<setup command>`, then `<test command>`. |
| S-tests-no-sandbox | the tests don't run until you ask: without a sandbox they'd run with your rights. |
| S-tests-cloned | the tests don't run until you ask: this repo was just cloned. |
| D8-say | each ticket runs a planner, a builder and a reviewer on `<id>`. you approve the plan and decide at handoff. When the reviewer differs: each ticket runs a planner and a builder on `<id>`, and a reviewer on `<reviewer id>`. you approve the plan and decide at handoff. |

S-hooks names the checks: the tests part's command and "no secrets" for the line
and reviewed templates, and the names of the template's `checks` with `runsOn:
'commit'` for a saved one. It shows only when `info.git` is true. In
S-tests-auto, the setup part and its comma are left out when there's no setup
command.

The level sentence uses the window's `level` for the line template. For a saved
template it uses the level of the template's own default runner
(`runners.find((r) => r.id === defaults.runner)`), or S-worktree when it has
none. Reviewed and open templates have no line, so they get no level sentence.
S-tests-* show only for the line template, the only one whose create can start a
test run.

**Test run** (written by the engine into the run's `text`; the window shows
the result word, then `text`, then `detail` in a collapsible block)

| Id | Result word | Text |
| --- | --- | --- |
| T-started | running… | Tests started on a copy of `<branch>`'s last commit. You can write a ticket meanwhile. |
| T-passed | passed | Tests passed on a copy of `<branch>`'s last commit. |
| T-failed | failed | Tests failed on a copy of `<branch>`'s last commit. While they fail, every commit in this repo is stopped, yours and the agents', because they run on each commit. Make them pass on `<branch>`, or change when the tests part runs, in setup. |
| T-setup-failed | failed | `<setup command>` failed on a copy of `<branch>`'s last commit, so agents won't start either: their worktrees run it first. Fix the command in setup, under the default runner. |
| T-cant-copy | couldn't tell | Couldn't make a copy of the last commit to test. |
| T-cant-start | couldn't tell | Couldn't start the tests in the sandbox. |
| T-cant-long | couldn't tell | The tests ran for 30 minutes and were stopped. |
| T-cant-closed | couldn't tell | The app closed before the tests finished. |
| T-not-run | not run | (window only) run the tests on a copy of the last commit to see whether tickets can pass them. |
| T-none | (none) | (window only) no test command, so tickets aren't tested before review. add a tests part in setup. |
| E-running | (error) | The tests are already running. |
| Action | | `[r] run tests` |

The engine's run-log sentences are full sentences with a capital letter, like
the gate's (`flow.rs:622`). For couldn't tell, the error text goes in `detail`
("no error text" when there is none), and T-cant-closed's `detail` is "The app
stopped during the run."

**Engine**

| Id | Text |
| --- | --- |
| F-protected-added | This protected test is new. The gate ran it as the branch wrote it. |
| P-no-branch | The project folder has no branch checked out, so Sawtooth couldn't add the hook that stops agents moving a branch from their worktrees. Check out a branch, then open the app again. |
| P-guard-hookspath | This repo keeps its git hooks in `<path>` (core.hooksPath), so Sawtooth couldn't add the hook that stops agents moving `<branch>` from their worktrees. |

**Work notes**

| Id | Text |
| --- | --- |
| D4-say | it shows here and on home when it needs you, not outside the app: for that, add a slack channel in setup. |
| N-approve-plan | read the plan. approving starts `<next>`. D4-say |
| N-approve-empty | the planner found nothing unclear, so there's no plan. approving starts `<next>` with your ticket as written. D4-say |
| N-accept-tests | it changed or added `<files>`, which the gate protects. accept if the ticket asked for that: the gates then run its versions. |
| N-gate-failed | a check at `<stage id>` failed (`<check names>` ran). send back gives build the output; let it through skips this gate. |
| N-agent-failed | `<stage id>` stopped without finishing. its terminal shows why, such as a harness that isn't signed in. open it, then send it back or drop it. |
| N-handoff-findings | review raised `<n>` high finding(s). merging anyway keeps them; send back gives them to build. D4-say |
| N-handoff | merging puts this branch into `<target>`, after the gates run once more. D4-say. When the stage has a merge command: merging runs the gates again, then your merge command. D4-say |
| N-queued | waiting for room: this factory runs `<at_once>` ticket(s) at once. |
| N-sent-back | this was sent back, so `<stage id>` runs again with what it was given. |
| N-gate-running | running `<check names>` on a clean copy of the branch. |
| N-agent-running | `<stage id>`: `<agent description>`. then `<next>`. |
| Home | a factory works on one project. give it tickets, and agents plan, build and review each one in their own copy of your repo. you approve the plan and decide whether to merge. a new factory starts with that line, and you can change it. |

"ticket(s)" and "finding(s)" are singular for 1 and plural otherwise, as in
`Work.tsx:591`. Race and panel stages have no single agent, so they always take
the empty-description form. `<at_once>` is `factory.defaults.atOnce`, which a new line
factory sets to 2 (`templates.rs:29`); when it's 0, N-queued is "waiting to
start." `<agent description>` is the agent's `description`
(`templates.rs:38-40`); when it's empty, the note is `<stage id> is working.
then <next>.` When there is no `<next>`, the "then" part is left out.

## Alternatives Considered

### Alternative A: A guided onboarding mode (tour, wizard or separate route)

A first-run flag switches on a step-by-step guide through each screen and part.

*   **Pros**: Easy to point at as "the onboarding". Can explain every concept in
    order. Doesn't change the screens everyone uses.
*   **Cons**:
    *   It explains the app as it is, including the promises that aren't true.
    *   It's a second UI to keep in step with New factory and Work.
    *   It needs a "first run" flag. A new field in `settings.yaml` or
        `factory.yaml` breaks older builds, which reject unknown fields
        (`app/src-tauri/src/settings.rs:29-31`).
    *   Every e2e test starts from an empty Home and would need a way to skip it.
    *   The help is gone by the time the first ticket stops, which is when it's
        needed.
*   **Reason for Rejection**: It adds a layer instead of fixing the screens that
    cause the problem.

### Alternative B: Teach every feature up front

Onboarding covers factories, harnesses, knowledge, checks, triggers, sources,
races, panels and the rest before the first ticket.

*   **Pros**: Nobody misses a feature.
*   **Cons**: Concepts taught before they're needed don't stick. It turns a first
    session into a course.
*   **Reason for Rejection**: The first session only needs the line. The docs
    website covers the rest.

### Alternative C: Four small edits only

From the concept review: turn New factory's folder note into a checklist, add
one sentence about worktrees, rewrite Home's empty copy, and show each running
agent's description. No engine changes.

*   **Pros**: Small. Helps every user.
*   **Cons**: The first ticket in a JS repo would still fail at the builder's
    first commit for missing dependencies, and loop on tests that already fail.
    The statements on screen would still be untrue for non-git folders and for
    the commit hook.
*   **Reason for Rejection**: This design is those four edits plus the three
    engine fixes that make them true: a setup command, the test run, and the
    finding text. Each engine fix removes a first-ticket failure the edits
    alone can only describe.

### Alternative D: A bundled sample project

Onboarding offers a small sample repo with offline tests.

*   **Pros**: Predictable. Removes the fear of changing a real repo.
*   **Cons**: A repo to ship and keep working. It still needs a signed-in
    harness and real tokens, and it hides the real setup failures until the
    first real ticket.
*   **Reason for Rejection**: Deferred. Decide after watching one real new user.
    The clone flow in New factory can already make a practice copy.

### Alternative E: OS notifications now (instead of D4's rewording)

Add `tauri-plugin-notification` and notify when a ticket waits on a person.

*   **Pros**: Makes "it tells you when it needs you" true with the window
    closed.
*   **Cons**: A new plugin, a macOS permission prompt in the middle of the first
    run, and choices about which events notify. It touches the reports system
    (`REPORT_EVENTS` in `app/src/api.ts`).
*   **Reason for Rejection**: Its own feature. Listed under Future Improvements.

### Alternative F: Copy the project's ignored folders into each new worktree

The first draft of this design copied ignored folders such as `node_modules`
from the project into each worktree when it was made.

*   **Pros**: No install step, and nearly free on APFS and reflink filesystems.
*   **Cons**:
    *   The project's `node_modules` may not match the ticket's lockfile.
    *   A secret kept in an ignored folder reaches agents.
    *   It's a full copy on other filesystems, for every worktree.
    *   It duplicates `setup_commands`, which already exists for this.
*   **Reason for Rejection**: The setup command installs from the ticket's own
    lockfile, under the agent's confinement, with no new engine code.

### Alternative G: A sign-in check now

Run one fixed prompt through each harness on the line after create.

*   **Pros**: Catches a signed-out harness before any stage runs.
*   **Cons**: A model call every time it runs. Telling sign-in errors apart from
    other errors means reading harness-specific output. It can't run on machines
    without a sandbox. With one harness on the line, the first stage catches the
    same problem.
*   **Reason for Rejection**: Deferred (D3).

### Alternative H: Stop counting tests a branch added as protected changes

The first draft left added files out of `changed_protected`.

*   **Pros**: No finding for a test the ticket was asked to write.
*   **Cons**: Removes a guard in every factory. A branch could add a test helper
    such as `conftest.py` that changes how existing tests run, with no person
    looking.
*   **Reason for Rejection**: The accept action already handles the case in one
    key press. Only the finding's text was untrue (D5).

### Alternative I: Always require `contained`

*   **Pros**: The strongest guarantee that agents can't move the branch.
*   **Cons**: Locks out machines without a sandbox, and anyone who chose
    `worktree` for a reason.
*   **Reason for Rejection**: D1 words the statement per level instead.

### Alternative J: Make the factory's hooks skip the person's own commits

*   **Pros**: The person's own commits wouldn't be stopped by the factory's
    checks, which surprises people.
*   **Cons**: The reviewed template exists for exactly that ("git hooks run the
    checks on every commit"), so it changes what that template is for.
*   **Reason for Rejection**: Out of scope. D1 discloses the hooks instead.
    Worth deciding on its own.

### Alternative K: Make the planner always write a plan

*   **Pros**: The approve step always looks the same.
*   **Cons**: Spends tokens on plans for tickets that don't need one, and
    changes behaviour for every existing factory.
*   **Reason for Rejection**: D6 explains the empty case in one sentence.

--------------------------------------------------------------------------------

## Risks & Mitigations

| Risk | Mitigation |
| --- | --- |
| The setup command itself fails (a stale lockfile, a missing tool). | The test run runs the same command first and shows T-setup-failed. In a worktree, the existing "the runner's setup failed. Its terminal shows why." stops the stage (`runner.rs:424`). |
| Under `contained`, the install needs a site that isn't in `network_allow`, such as a private registry. | The test run can't catch this: its setup runs with the network open. The first stage's install meets the web filter, which asks through the existing blocked-site request; "always" adds the site for later tickets. |
| The test run costs time: a full install and test suite once after create. | It runs in the background, says so before create (S-tests-auto), and doesn't block writing a ticket. |
| Failing tests stop every commit through the commit hook. | T-failed says so and names both fixes. The failure stays shown in Work's empty state until a test run passes. |
| Create runs the repo's code. | It's said before create, it runs only in the sandbox, and never on a just-cloned folder without the person asking. |
| A test command that never ends (watch mode). | `GATE_TIMEOUT` stops it after 30 minutes, with T-cant-long. |

## Non-Functional Requirements

*   **Security & Privacy**:
    *   The test run runs repo code only inside the sandbox, unless the person
        asks for it on a machine without one. Its setup step has the network
        open, as an install needs; its test step uses the tests part's network
        setting, as the gate does.
    *   Agents get dependencies from their own install, not a copy of the
        person's ignored files, so no `.env` or other ignored secret reaches
        them through this change.
    *   The test run's output goes in the runs log, which already holds check
        output (`hooks.rs:400-405`). It's capped at 4 KB per run
        (`app/src-tauri/src/runs.rs:11`).
*   **Performance & cost**:
    *   The test run costs one install and one test suite, after create and when
        asked. No model is called.
    *   Each new worktree runs the setup command once. For a JS repo that's two
        installs per ticket: plan runs first, in an isolated copy that has no
        "set up" marker to carry yet (`flow.rs:561`, `runner.rs:376-382`), and
        build's worktree then runs it again. Review's copy carries build's
        marker. The package managers' caches keep repeat installs short.
*   **Rollout & Rollback**:
    *   No settings or factory file changes shape. The setup command and sites
        live in the existing `Runner` fields, so older builds read them.
    *   Existing factories don't change, except for the added-test finding text.
        The setup command applies only to factories created after this change.

## Open Questions

*   **Switching branch mid-ticket.** A merge targets whatever branch the project
    folder has checked out when the merge runs (`flow.rs:1174`). The safety
    statement names the branch at create time. Whether the engine should pin
    the target branch per ticket is outside this design.

## Future Improvements

*   **OS notifications** for "needs you" events, so D4 can say more.
*   **A sign-in check** for a reviewer harness that differs from the builder's
    (D3).
*   **Factory hooks that skip the person's own commits** (Alternative J).
*   **A sample project or practice copy,** decided after watching one real new
    user (Alternative D).
*   **Pinning the merge target per ticket.**

## Additional Resources

*   [first-ticket-onboarding-brief.md](first-ticket-onboarding-brief.md): the
    concept review this design comes from. The Decisions section refers to its
    questions.
*   `PRODUCT.md`: users, brand and design principles.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

Work happens on a fresh branch, `feature/first-ticket-onboarding`, cut from an
updated `main`. Steps 1–8 are the engine and can land and be tested before the
window changes. Steps 11 and 17 land together, because folding New factory's
options breaks the e2e scripts step 17 fixes.

### Target Files to Create/Modify

**Engine**

#### 1. `app/src-tauri/src/gate.rs` (Modify)

*   **Type / Scope**: Rust module, gate snapshots.
*   **Proposed Changes**:
    *   Move the part of `snapshot` that removes an old copy and adds a detached
        worktree (lines 116-126) into a private `fn copy_of(project: &Path,
        head: &str, into: &Path) -> Result<Snapshot>`. `snapshot` calls it, and
        behaves as before.
    *   New `pub(crate) fn snapshot_head(project: &Path, into: &Path) ->
        Result<Snapshot>`: resolves `HEAD` in `project` and calls `copy_of`. It
        doesn't check for uncommitted changes and copies no ignored files.
    *   New `pub fn added(worktree: &Path, base: &str) -> Result<HashSet<String>>`:
        the paths from `git diff --name-only --no-renames --diff-filter=A base
        HEAD`. `std::collections::HashSet` joins the file's imports.
    *   `changed_protected` is unchanged, and its test at lines 205-207 still
        passes.
    *   Tests:
        *   `snapshot_head` works with an uncommitted change in the project, and
            the copy has the committed content;
        *   `snapshot_head` copies no ignored file;
        *   `added` lists a file the branch added and not one it changed.

#### 2. `app/src-tauri/src/project.rs` (Modify)

*   **Type / Scope**: Rust module, project detection.
*   **Proposed Changes**:
    *   `ProjectInfo` gains `commits: bool` and `setup_command: Option<String>`.
    *   `detect` sets `commits` from `crate::worktree::git(path, &["rev-parse",
        "--verify", "--quiet", "HEAD"]).is_ok()` when `git` is true, and false
        otherwise. (The `git` already imported in this file is the module
        `crate::git`, which has no general runner.)
    *   New `pub struct Setup { pub command: String, pub sites: Vec<String> }`
        and `pub fn setup(path: &Path) -> Option<Setup>`, from the D2 table:
        only when `package.json` exists, checking lockfiles in the table's
        order. `detect` sets `setup_command` from it.
    *   Tests:
        *   `git init` with no commit gives `git: true, commits: false`;
        *   each lockfile gives its row's command and sites;
        *   a `package.json` with no lockfile, and a folder with only a
            lockfile, give `None`.

#### 3. `app/src-tauri/src/templates.rs` (Modify)

*   **Type / Scope**: Rust module, the default line.
*   **Proposed Changes**: `standard_line` gains `setup: Option<&project::Setup>`.
    When it's `Some`, the default runner gets `setup_commands: vec![command]`
    and `network_allow: sites`. The four existing tests at lines 170-191 pass
    `None` (three tests hold four calls). New test: a `Setup` for `npm ci` lands
    on the default runner with `registry.npmjs.org`.

#### 4. `app/src-tauri/src/flow.rs` (Modify)

*   **Type / Scope**: Rust module, gates.
*   **Proposed Changes**:
    *   New `pub(crate) fn run_check(&self, dir: &Path, command: &str, network:
        bool) -> Result<(bool, String), String>` on `Engine`: `confined_check`,
        then `hooks::run_command` with `GATE_TIMEOUT`. `Err` means the check
        couldn't start. `checks_in` (lines 613-620) and `bench_passed`
        (`bench.rs:105-118`) call it in place of their own copies, with the
        same results as today.
    *   New `const PROTECTED_ADDED` holding F-protected-added. At line 707, the
        finding code calls `gate::added(worktree, &w.base)` once, and uses
        `PROTECTED_ADDED` for files in that set and `PROTECTED_CHANGED` for the
        rest. If `added` fails, every file gets `PROTECTED_CHANGED`, as today.
    *   `accept_tests` (line 1041) rewrites findings that start with either
        `PROTECTED_CHANGED` or `PROTECTED_ADDED`. Without this, an added-file
        finding would stay high after accepting.
    *   Tests: a branch that adds `tests/new.test` gets the new text, and one
        that changes an existing protected file gets the old one; accepting
        clears an added-file finding to low.

#### 5. `app/src-tauri/src/testrun.rs` (Create)

*   **Type / Scope**: Rust module, the test run (D2), as `impl Engine`.
*   **Proposed Changes**:
    *   `pub fn run_tests(self: &Arc<Self>, id: &str) -> Result<(), String>`:
        *   finds the factory, and its part with id `tests`, kind check and a
            non-empty command. That's the part `create_factory` makes
            (`main.rs:340-346`) and the one the line's verify stage names.
            Without it: `Err("This factory has no tests part.")`;
        *   adds `id` to `State.testing`, or returns `Err` with E-running if
            it's already there;
        *   logs T-started, then starts a thread named
            `sawtooth-test-run`, and returns.
    *   The thread holds a guard whose `Drop` removes `id` from
        `State.testing`, so a panic can't leave it set. It:
        1.  calls `gate::snapshot_head` into `state_dir(id)/gates/test-run`;
            an error logs T-cant-copy with the error in `detail`;
        2.  runs each of `runner::pick(&f, None)`'s `setup_commands` through
            `run_check` with `network: true`, stopping at the first that fails;
        3.  runs the part's command through `run_check` with `part.network`;
        4.  logs one result run.
    *   `fn outcome(setup: Option<Result<(bool, String), String>>, tests:
        Option<Result<(bool, String), String>>, branch: &str, setup_command:
        &str) -> Run`: pure, so it can be tested. `Ok((true, _))` from the
        tests gives T-passed. `Ok((false, out))` gives T-failed, or
        T-cant-long when `out` contains `hooks::STOPPED`. A failed setup gives
        T-setup-failed. `Err(e)` from either gives T-cant-start, with `e` in
        `detail`. `detail` holds `runs::last_lines(&out, 40)`.
    *   `pub fn latest(store: &Store, id: &str) -> Option<Run>`: the newest
        test-run record in the whole log, from `runs::read(store, id,
        usize::MAX)`. It isn't limited to the newest 200 runs, because a busy
        factory logs more than that between test runs.
    *   `pub(crate) fn close_test_runs(&self)`: for each factory, when
        `latest` is T-started (`ok` and `detail` both absent), logs
        T-cant-closed. Called from `Engine::start` before `runs_seen` is
        filled, so the window hears of it.
    *   Every run it logs has `kind: "check"`, `part: "tests"` and `by:
        "engine"`, and no `work`.
    *   Tests:
        *   `outcome` for passed, failed, timed out, setup failed, couldn't
            start, and no setup;
        *   `close_test_runs` logs T-cant-closed after an open start, and
            nothing after a result or after a T-cant-closed (whose `detail` is
            set);
        *   `latest` finds a test-run record with 300 other runs after it;
        *   a second `run_tests` while one is running returns E-running.

#### 6. `app/src-tauri/src/engine.rs` (Modify)

*   **Type / Scope**: Rust module, engine state.
*   **Proposed Changes**: `State` gains `pub(crate) testing: HashSet<String>`,
    "factories whose test run is going right now". (The existing `checking`
    field is for racers.) `State` derives `Default`, so `Engine::new` needs no
    change. `start` calls `close_test_runs` first.

#### 7. `app/src-tauri/src/hooks.rs` (Modify)

*   **Type / Scope**: Rust module, git hooks.
*   **Proposed Changes**:
    *   New `pub(crate) const STOPPED: &str = "(stopped: it ran too long)"`,
        used by `run_command` at line 348 in place of the literal.
    *   `install_guard`: when no branch is checked out, return
        `Some(P-no-branch)` instead of `None`.
    *   `install`: when `hooks_dir` fails (lines 171-179) and the factory has a
        line, the returned problems also hold P-guard-hookspath, whether or not
        any check needs a hook.
    *   Tests: a detached `HEAD` gives a problem that contains "no branch
        checked out"; `core.hooksPath` on a line factory with no commit checks
        gives P-guard-hookspath.

#### 8. `app/src-tauri/src/main.rs` (Modify)

*   **Type / Scope**: Tauri commands.
*   **Proposed Changes**:
    *   `mod testrun;` next to `mod bench;` (line 23), where the other `impl
        Engine` modules are declared.
    *   `create_factory` gains `engine: State<'_, Arc<Engine>>` (as at line 710)
        and `run_tests: Option<bool>`. Then:
        *   **the git rule:** before anything is created, when
            `from.as_ref().map_or(template == "line", |t| !t.line.is_empty())`
            and the folder isn't a git repo with a commit, return
            `Failure::new("git", …)` with R-git and its fix;
        *   **the level:** only the fallback arm changes: `_ =>` becomes
            `Level::Contained` when `sawtooth_sandbox::available()` is `Ok`,
            and `Level::Worktree` otherwise. `Level`'s serde default doesn't
            change;
        *   **the setup:** `project::setup(folder).as_ref()` is passed to
            `standard_line`;
        *   **the test run:** after the "became a factory" run is logged, when
            `run_tests == Some(true)`, `template == "line"`, `from` is `None`,
            the sandbox is available, `SAWTOOTH_NO_AUTO_TESTS` isn't set, and
            the factory has a tests part, call `engine.run_tests(&factory.id)`
            and ignore its result.
    *   New commands, registered in `generate_handler!`:
        *   `run_tests(engine, id) -> Result<(), Failure>`, mapping `Err` to
            `Failure::new("tests", …)`;
        *   `test_run(app, id) -> Option<Run>`, returning `testrun::latest`.

**Window**

#### 9. `app/src/api.ts` (Modify)

*   **Type / Scope**: TypeScript API types and calls.
*   **Proposed Changes**:
    *   `ProjectInfo` gains `commits: boolean` and `setupCommand: string |
        null`.
    *   `createFactory`'s argument gains `runTests?: boolean`.
    *   New `api.runTests(id)` and `api.testRun(id): Promise<Run | null>`.

#### 10. `app/src/factory/TestRun.tsx` (Create)

*   **Type / Scope**: React component and a pure helper.
*   **Proposed Changes**:
    *   `export function testRunOf(run: Run | null): 'notRun' | 'running' |
        'passed' | 'failed' | 'cantTell'`, read as in D2: null → not run;
        `ok === true` → passed; `ok === false` → failed; no `ok` and no
        `detail` → running; no `ok` with `detail` → couldn't tell.
    *   `TestRun({ factory })`: calls `api.testRun(factory.id)` on mount and
        again whenever the store's `runs[factory.id]` changes (the store
        reloads that on `factory:runs`), and keeps the answer in local state.
        It shows the
        result word, the run's `text` (or T-not-run), the time (`clock(run.at)`),
        `detail` in a collapsible `<pre>` for failed and couldn't tell, and
        `[r] run tests`. The action calls `api.runTests` and shows a refused
        run's message through `showProblem`. It's disabled while `running…`.
    *   Only the existing muted and red text classes; no motion.

#### 11. `app/src/NewFactory.tsx` (Modify)

*   **Type / Scope**: New factory screen.
*   **Proposed Changes**:
    *   **Defaults:**
        *   `reviewer` is `reviewerTyped ?? harness`;
        *   `level` becomes `levelTyped ?? (sandbox === null ? 'contained' :
            'worktree')`, with `sandbox` from a new `useEffect` that calls
            `api.machine()` once (`undefined` until it answers). While it's
            `undefined`, `whyNot` is "checking this machine's sandbox…", as in
            "Defaults in New factory";
        *   a new `cloned` state, set to true in `onCloned` (line 261) and in
            `create()`'s clone branch (line 89), and back to false when the
            folder is changed any other way. Both clone paths then count.
    *   **Fold:** the fields listed in "Defaults in New factory" go in one
        group with `hidden={!unfolded}`. The summary line above it uses the
        summary formats in the Copy table, with a `[c] change` button. `c` is
        added to the second `useKeys` block (line 137) and to the screen's key
        hints. `o` keeps toggling the folder picker.
    *   **Quick checks:** a list under the folder field, from the Quick checks
        table. The note at lines 404-406 moves here as T-none-nf. The `note`
        and `tags` lines (151-161) stay.
    *   **Create rule:** `whyNot` gains R-git under the condition in "Create
        rules".
    *   **Safety statement:** the tree (lines 172-189) shows S-hooks for
        `line`, `reviewed`, and saved templates with commit checks, in place of
        the reviewed-only line; then the level sentence, chosen as in the Copy
        section (or S-no-branch when `info.branch` is null); then, for the
        line template with a test command, S-tests-cloned when `cloned ||
        willClone`, else S-tests-no-sandbox when `sandbox` isn't null, else
        S-tests-auto. These need `info.git`; without it the tree shows only
        what it shows today.
    *   **Create:** passes `runTests: template === 'line' && !cloned &&
        !willClone && sandbox === null`.
    *   **The line option's note** is D8-say.

#### 12. `app/src/factory/Work.tsx` (Modify)

*   **Type / Scope**: Work screen and ticket detail.
*   **Proposed Changes**:
    *   **Empty state:** as in "Work's empty state", in the right-hand column
        (line 254) when `flat.length === 0 && canWrite`.
    *   **Notes:** a new exported `noteFor` in this file, with the inputs and
        rules in "Stage notes in Work", replacing the `say` assignments in
        lines 607-643. The `verbs` arrays don't change.
    *   **Accepting tests:** `testChanges` (line 599) matches findings whose
        text starts with `This protected test`, so both the changed and the
        added texts count. `acceptSay` is replaced by N-accept-tests.

#### 13. `app/src/factory/Setup.tsx` (Modify)

*   **Type / Scope**: Setup screen.
*   **Proposed Changes**: A new section, `['tests', 'test run', undefined]`,
    after proposals and before general in `sections` (line 128), shown only
    when the factory has a part with id `tests`. It renders `TestRun`.
    Proposals stay first, because they need an answer.

#### 14. `app/src/Home.tsx` (Modify)

*   **Type / Scope**: Home screen.
*   **Proposed Changes**: The empty-state paragraph becomes the Home text in the
    Copy table. It keeps the one "+ new factory" action.

#### 15. `app/src/factory/FactoryMap.tsx` and `app/src/harness.ts` (Modify)

*   **Type / Scope**: Map node text and a harness helper.
*   **Proposed Changes**:
    *   `lastRun` (`FactoryMap.tsx:313`) skips runs with `ok === undefined`.
        Without that, a test run that has started, or couldn't tell, shows as
        "last: pass" on the tests node (line 334).
    *   `harness.ts` exports `program`, for the quick checks.

**Docs**

#### 16. `PRODUCT.md` and the factory's `knowledge.md` (Modify)

*   **Type / Scope**: Product notes and project knowledge.
*   **Proposed Changes**:
    *   `PRODUCT.md`, Users: a new engineer's first ticket shapes the product
        too. They should get from install to a first ticket at handoff without
        reading anything outside the app.
    *   `knowledge.md` (which `CLAUDE.md` links to), two lines under "About this
        project":
        *   a new line factory's default runner gets its setup command and
            registry sites from the JS lockfile (`project::setup`);
        *   the test run is recorded in the runs log as `kind: "check"`, `part:
            "tests"`, `by: "engine"`, with no `work`.

**Real-app tests**

#### 17. `e2e/harness.py`, `e2e/new_factory.py`, `e2e/work_screen.py`, `e2e/templates.py` (Modify)

*   **Type / Scope**: e2e harness and the tests that use New factory's folded
    fields.
*   **Proposed Changes**:
    *   `harness.py` adds `SAWTOOTH_NO_AUTO_TESTS='1'` to `env` (line 14). That
        keeps all 24 scripts that call `create_factory` from starting a test
        run.
    *   `new_factory.py`, `work_screen.py` and `templates.py` press `c` before
        clicking or reading text inside the fold. `templates.py:41-43` reads
        `innerText`, which is empty while hidden.
    *   `new_factory.py` also checks that the reviewer defaults to the builder's
        harness.
    *   `clone_repo.py` needs no change: it reads `#f-name`'s `value`, which a
        hidden field keeps.

#### 18. `e2e/first_ticket.py` (Create)

*   **Type / Scope**: e2e test, run by hand with a built app.
*   **Proposed Changes**:
    *   Uses `build_standin('line')`. Its `turn.sh` makes the planner end
        without writing `PLAN.md`.
    *   The project is a git repo with a `Makefile` whose `test:` target runs
        `true`, so the test command is `make test`.
    *   New factory is checked through the screen, but the factory is created
        with `inv('create_factory', …, harness=ROOT + '/bin/claude')`, as
        `e2e/isolation.py` and others do. The screen's default harness is the
        first installed one, the real `claude`, and no e2e test may call a
        model (`e2e/harness.py:4`).
    *   Pass conditions:
        *   empty Home shows the Home text;
        *   a non-git folder shows "git repo" failed, and create is disabled for
            a line with R-git;
        *   the git repo shows the quick checks passed and S-worktree or
            S-contained naming its branch;
        *   after create, Work shows T-not-run (the switch is set), and
            `inv('run_tests', id=…)` then gives `passed`;
        *   the first ticket shows N-approve-empty at approve.

#### 19. `e2e/work_states.py` (Modify)

*   **Type / Scope**: e2e test of the ticket detail's states.
*   **Proposed Changes**: Assert the note text for rules 5, 6, 7, 11, 14 and 15
    of the stage notes table, using the states this script already drives,
    plus: a branch that adds a protected test shows N-accept-tests, and
    accepting clears it; and a factory with a `core.hooksPath` repo shows
    P-guard-hookspath in Work's empty state.

### Verification Plan

*   **Compilation/Build**: `cargo build --workspace`, and `cd app && npx tsc
    --noEmit`.
*   **Unit Tests**: `cargo test --workspace`, including the new tests in
    `gate.rs`, `project.rs`, `templates.rs`, `flow.rs`, `testrun.rs` and
    `hooks.rs`.
*   **Static Analysis & Linting**: `cargo clippy --workspace --all-targets`.
*   **Test Coverage Requirement**: every new Rust function has a test, and each
    branch of `outcome` has one.
*   **E2E/Integration**: by hand, with a built app and open windows (not inside
    a ticket): `e2e/first_ticket.py`, `e2e/new_factory.py`,
    `e2e/clone_repo.py`, `e2e/work_screen.py`, `e2e/templates.py` and
    `e2e/work_states.py`. `clone_repo.py` also checks that a repo cloned with
    the clone panel's button shows S-tests-cloned.
*   **Manual check**: one real first run on a JS repo with a
    `package-lock.json` and passing tests, at the `contained` level. Pass
    condition: the builder's first commit isn't stopped by the commit hook, and
    verify passes without sending the ticket back.
