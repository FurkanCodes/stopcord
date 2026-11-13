# First-run onboarding: one real ticket through the default line

* **Author:** Furkan
* **Date:** 2026-11-12

## Overview

When Stopcord opens with no factories, this feature takes a new user from the empty Home screen to one small ticket in their own repo going through the default line (plan, approve, build, verify, review, handoff). The guided steps are:

1. pick a folder
2. pass a setup check (harness installed and signed in, folder is a git repo, test command runs), with a fix shown for each failure
3. read what the factory will do to the repo
4. write a small ticket
5. approve the plan

After approval, the app says where it will show what needs the user. Each stage explains itself in place when the ticket reaches it. The first run ends when the ticket reaches an end. A practice copy or sample project is offered only to people who won't use their own repo.

The target user is an engineer who already uses a coding-agent CLI and has never used Stopcord. The value claim is that "nothing to look up, nothing fails silently, every step says what happens next" gets them through a first ticket without a docs site.

Six review lenses looked at the concept: design, codebase details, zoom-out, comprehension, implementation readiness and structure. They agree on one thing. Four of the concept's promises are not true of the code today:
- "the test command runs"
- "logged in"
- "nothing reaches your branch until handoff"
- "it will tell you when it needs you"

Those four have to be made true, or reworded, before any onboarding copy is written.

## Analysis

### Risks and weak points, most serious first

**1. "Nothing reaches your branch until handoff" is not true as worded.** All six lenses raised this.
- **Non-git folders get no isolation.** `worktree::ensure` returns the project path itself (`app/src-tauri/src/worktree.rs:42-46`), and `prepare_copy` returns `None`, so even the plan stage has no isolation (`app/src-tauri/src/flow.rs:548-553`). New factory allows such a folder, with only a dim note: "not a git repo, so terminals share one folder" (`app/src/NewFactory.tsx:155-156`).
- **A repo with no commits** fails outright (`worktree.rs:47-51`).
- **The read-only branch ref is only enforced at the `contained` level** (`crates/sandbox/src/paths.rs:70-80`, `app/src-tauri/src/runner.rs:129`). The e2e proof runs only at that level (`e2e/main_guard.py:12`).
- **At the `worktree` level the only guard is a `reference-transaction` hook** (`app/src-tauri/src/hooks.rs:109-131`). It has gaps:
  - It only fires when the agent is working inside the worktrees folder.
  - It is skipped when the repo sets `core.hooksPath`, as husky does (`hooks.rs:76-84,171-179`).
  - It is skipped on a detached HEAD, without saying so (`hooks.rs:136`).
  - It is skipped when the repo already has a hook of that name (`hooks.rs:138-139`).
- **The defaults disagree.** The window defaults to `contained` (`NewFactory.tsx:38`). The backend and the enum fall back to `worktree` (`app/src-tauri/src/main.rs:366-371`, `app/src-tauri/src/factories.rs:116-117`).
- **The runner field makes "worktree" look like an option.** It lists "worktree: its own folder and branch" as an alternative to the default (`NewFactory.tsx:399-400`). A new user will read that as "the default isn't in a worktree".

**2. Creating the factory changes the repo before any ticket exists.**
- **One tracked file is edited.** An existing CLAUDE.md or AGENTS.md gets a pointer block appended, which shows as an uncommitted change on the user's branch (`app/src-tauri/src/kb.rs:4-6,164-174`; the UI admits it at `NewFactory.tsx:158`).
- **Hooks go into the shared `.git/hooks`.** `pre-commit` and `pre-push` hooks are added (`hooks.rs:55,61-74,153-226`). `run_hook` doesn't filter by folder (`hooks.rs:355-385`). So the user's own commits in their main checkout then run the factory's checks, including the full test command, with a 30-minute limit (`hooks.rs:22`; checks added at `main.rs:338-357`).
- **The safety statement comes too late.** The concept places it "before the first ticket", which is after these changes. The current copy "nothing is committed or pushed" (`NewFactory.tsx:454`) is true, but it leaves out the hooks.
- **Open question for the user.** One lens asked whether the uncommitted CLAUDE.md edit can make the handoff merge fail, since the merge runs in the project folder. Nobody checked this.

**3. The setup check measures the wrong thing for tests.**
- **The command is never run.** `project::test_command` only guesses from file names (`app/src-tauri/src/project.rs:52-84`):
  - any `pyproject.toml` becomes `pytest`, even under uv or poetry, or with pytest not installed;
  - an `npm init` stub (`echo "Error: no test specified" && exit 1`) is taken as a test command;
  - a watch-mode `test` script would hang until the gate's 30-minute limit (`flow.rs:23`).
- **The gate runs somewhere else.** Passing in the user's checkout doesn't predict the `verify` gate. The gate runs in a fresh snapshot, under the sandbox, with no network (`flow.rs:89-102`, `app/src-tauri/src/gate.rs:107-127`). It copies ignored folders such as `node_modules` from the worktree, and new worktrees start without them (`gate.rs:54-77`, `worktree.rs:86-90`). The default runner has no `setup_commands` (`app/src-tauri/src/templates.rs:31-36`).
- **Tests that already fail cost a lot.** If tests already fail on the base branch, verify loops back to build up to 3 times (`templates.rs:61-68`) and ends as `failed`. The user's first ticket then spends three builder runs on a failure they never caused.
- **No test command means no verify stage.** The line then has no tests gate at all (`templates.rs:58-69`, `NewFactory.tsx:404-406`). The concept's "tests gate" stage may not exist for these users.

**4. Nothing checks "logged in".**
- **"Installed" is only `command -v`.** It runs in a login shell with an 8 s timeout and returns an empty list on any failure (`main.rs:262-266`, `app/src-tauri/src/shell.rs:40-93`). A slow `.zshrc` therefore reads as "no harness installed".
- **There is no login check of any kind.** `login_env` and `login_hint` exist only for VMs (`app/src-tauri/src/harness.rs:70-75,126-127`). Screen detection has no rule for login screens (`crates/detect/rules/*.toml`), so a harness that isn't signed in just looks stuck at plan.
- **Per-harness checks would break a product rule.** PRODUCT.md:36 says "No screen assumes one". So the check has to come from harness profile data, and "can't tell" has to be a valid result.
- **The nearest building block is `Engine::one_shot`** (`app/src-tauri/src/oneshot.rs:81-108`). A "reply ok" ping through it would prove sign-in, network-filter access and Keychain access from inside the sandbox, all at once. Its limits:
  - it needs a saved factory;
  - it needs sandbox support;
  - it needs the harness to have a one-prompt command;
  - it costs a model call and can take up to 120 s.
- **The reviewer defaults to the second installed harness** (`NewFactory.tsx:36-37`). A user with codex installed but never signed in will fail at review, after all the earlier work is spent.
- **Trust prompts stop the first plan at the `worktree` level.** There, the "trust this folder" prompts of Claude and Codex are not answered automatically (`harness.rs:128-133,156-161`).

**5. "It will tell you when it needs you" has no channel outside the window.**
- **Notifications exist only inside the app.** The only Tauri plugin is the file dialog (`app/src-tauri/Cargo.toml:23`). The factory's notification setting says "only inside stopcord" (`app/src/factory/Setup.tsx:500`).
- **Closing the window hides it, and the engine keeps working** (`main.rs:1152-1158`).
- **"Needs you" shows only in Home's "next up" and in Work** (`app/src/Home.tsx:199-214`, `app/src/store.ts:409-419`).
- The promise fails exactly when the user has stepped away, which is what the concept tells them they can do.

**6. The first run is unlikely to follow the happy path, and the stops come in the unguided half.**
- **A protected-test finding.** The builder is told to write a failing test first (`templates.rs:20`). The gate protects `tests/**` and `**/*.test.*` (`templates.rs:64`). `changed_protected` counts new files as changed (`gate.rs:50`). So a well-behaved first ticket will likely raise "This protected test was changed. The gate ran the base branch's copy." (`flow.rs:31`), with an "accept its test change" button (`Work.tsx:600-601`). A first-time user can't tell what that means.
- **An empty plan.** The planner is told "If nothing is unclear, say so and stop" (`templates.rs:18`). A small first ticket is the likeliest case for that. Approve then shows "the planner didn't write a plan" above "[a] approve plan" (`Work.tsx:608,704-712`). The one guided decision may be approving nothing.
- **Other stops:**
  - blocked-site requests from the `contained` web filter, e.g. `npm install` asking for registry.npmjs.org (`factories.rs:129-132`, `Work.tsx:285-294`);
  - an agent asking a question in its own terminal (`Work.tsx:24-26,34`);
  - review sending findings back to build up to 2 times (`templates.rs:70-76`);
  - a merge conflict (`flow.rs:1175-1182`);
  - a dropped first ask at start-up (commit 1a282b7).

**7. The concept may be solving a problem without evidence.**
- PRODUCT.md:13 says the first user is the person building the app, and that other engineers "don't shape the first version".
- The only evidence in the repo is commits 3961f7e ("Fixes from the first real use") and 1a282b7. Both are about the line failing silently or confusingly, not about a user not understanding the stages.
- The zoom-out lens argues that "every step says what happens next" is a property the line and the setup screen should always have, not an onboarding mode.
- The path already exists in about three clicks: Home empty state (`Home.tsx:186-197`) → New factory with the line as default (`NewFactory.tsx:31`) → Work (`NewFactory.tsx:117`) → "press w to write a ticket" (`Work.tsx:215-224`) → approve, which says "build starts when you approve" (`Work.tsx:609`).

**8. Detecting "first launch" from Home's look, or storing it in a config file, is fragile.**
- **Nothing persists.** Window state is in memory (`store.ts:61-77`).
- **`settings.yaml` and `factory.yaml` reject unknown fields** (`app/src-tauri/src/settings.rs:29-31`, `app/src-tauri/src/definition.rs:112-115`). An `onboarded` field would make an older build fail to read the file after a downgrade, switching off Slack and keep-awake without a word (`app/src-tauri/src/intake.rs:36,168`).
- **The `Stage` and `Waiting` enums are strict too** (`factories.rs:362-390`). A new waiting value breaks reading the whole factory on an older build.
- **Home's empty state is an unreliable trigger.** It needs `cards.length === 0` (`Home.tsx:186`), and any stray terminal outside a factory adds an "other terminals" card (`Home.tsx:56`).
- **"Zero factories" can be wrong.** Unreadable factories are left out of the list, and a failed load still sets `loaded: true` with an empty list (`store.ts:208-219`). Either would show onboarding to an existing user.
- **A "flag missing" rule shows onboarding to everyone who upgrades.**

**9. The concept relies on words the UI never defines.**
- **Terms with no definition:** factory, line, harness, reviewer (a harness picker labelled like a person), runner, contained, vm, link as, engine, knowledge, sources, gate, handoff, landing, findings. Home's one definition of "factory" adds three more undefined terms, one of them out of scope ("triggers", `Home.tsx:189`).
- **One moment has three names.** The concept calls the tests stage "tests gate"; the UI calls it `verify` (`templates.rs:67`). The end moment is called handoff, merge and approve (`Work.tsx:18,275,612`).
- **The New factory form asks for about ten decisions before anything happens** (`NewFactory.tsx:235-433`).
- **Keyboard-first prompts** ("press w", "[6] setup") assume the user has learned the hint bar.

**10. Problems the engine already finds are hidden during first run.**
- `create_factory` ignores what `app.apply` returns (`main.rs:384`).
- Those problems show only in the map's side pane when a part is selected (`app/src/factory/SidePane.tsx:124-126`). Examples: a husky `core.hooksPath`, an existing pre-commit hook, no branch guard.
- New factory opens Work, not the map. So the user never sees them, which breaks "nothing fails silently".

**11. The two waits show little.**
- **Before the plan:** the wait for the plan is a dead zone (`Work.tsx:331-334`, `engine.rs:870`).
- **After approval:** it shows only "build is working" (`Work.tsx:641`), with:
  - no stages left;
  - no time spent;
  - no sign of a stall;
  - "when there's room" never defined (`Work.tsx:385`, `at_once: 2` at `templates.rs:29`).
- **Cost:** a small ticket runs at least three agent sessions, possibly on two harnesses, plus loops. This is an estimate, not measured: tens to a few hundred thousand tokens.

**12. Tests and measurement.**
- **Every e2e test starts from an empty home** (e.g. `e2e/clone_repo.py:7,13-14`). An onboarding that takes over Home or the `+` key breaks most suites unless they have a way to skip it.
- **There is no telemetry.** The only record is the per-factory run log (`app/src-tauri/src/runs.rs:110`), and it doesn't exist before a factory is created. So setup check results leave no trace.

**13. The sample project is costly and adds little.**
- **Cost:** a bundled repo with tests that run offline, kept up to date. It still needs a signed-in harness and real tokens, so it removes none of the real setup failures.
- **Two cheaper options were raised:**
  - a local "practice copy of your repo", made with a `git clone` into a sibling folder (`app/src-tauri/src/clone.rs`);
  - pasting any public repo into the existing clone flow (`NewFactory.tsx:461-524`).

### What already exists and can be reused

- **Folder step:** New factory's nearby-folder picker, browse and clone, `detectProject` (git, branch, harness files, test command), and the "will create" pane (`NewFactory.tsx:62-76,151-189,449-456`; `project.rs:34-42,88-139`).
- **Check UI:** the Integrations `Step` component and `ConnectionState` (`app/src/Integrations.tsx:209,339-343`).
- **Check result shape:** `probe::Check` (`app/src-tauri/src/probe.rs:45-53`).
- **Sandbox checks:** `test_runner` (`main.rs:697-701`, needs a factory) and `stopcord_sandbox::available()` (`crates/sandbox/src/wrap.rs:20`).
- **Writing the ticket:** `Compose` in Work (`Work.tsx:307-390`).
- **Explaining each step:**
  - Detail's per-state `say` line (`Work.tsx:603-643`);
  - the `waitingWord` table (`Work.tsx:14-21`);
  - `LineBar` (`Work.tsx:262-283`);
  - the one-line agent descriptions (`templates.rs:38-40`: "writes the smallest plan that settles what's unclear", "makes the change", "reports problems, changes nothing").
- **Rollback:** removing a factory calls `hooks::uninstall` and `kb::unlink` (`hooks.rs:229-241`, `kb.rs:182`).

### Merit

- The concept's constraints fit the product: quiet, literal, nothing to look up.
- So does its decision to use the user's own repo.
- It also answers the real fear, "will this touch my branch?", up front.
- Most of the path exists already. This work is mainly making it honest and complete, not building a new UI.

## Goals vs. Non-Goals

* **In-Scope Goals:**
  * A user with no factories reaches a first ticket at handoff, or at another end, in their own repo. They don't need to read anything outside the app.
  * A setup check with four results per item (pass, fail with a fix, timed out, can't tell), recorded somewhere that survives:
    * machine checks: harness on PATH, harness signed in for both builder and reviewer, sandbox available;
    * project checks: git repo with at least one commit, branch guard installed, test command found and passing.
  * A trust statement built from detected state, shown in New factory before create. It lists what create writes (hooks, links, the CLAUDE.md line), names the runner level and target branch, and says what agents can't do until handoff.
  * One plain explanation for every state a first ticket can reach: running, question, blocked site, protected test, empty plan, failed gate, sent back, merge conflict, merged, dropped.
  * Copy that says where "needs you" will appear, and is true.
  * One vocabulary for first run, matching the on-screen labels (verify, not "tests gate").
  * A way for e2e tests to skip onboarding.
* **Out-of-Scope Non-Goals:**
  * Teaching features beyond the first run; the docs site covers those.
  * Triggers, races, panels, scorers.
  * General UI polish.
  * A separate onboarding route or wizard screen. All lenses that addressed it advise against one, because it would drift from New factory and Work.
  * Per-harness code paths that branch on harness names.
  * Telemetry sent off the machine.
  * Confetti, celebration or chirpy copy.

## Key Decisions & Staging for Next Steps

### Decisions the lenses agree on, to carry into design

1. **No new route.** The flow adds content to three existing screens: Home's empty state (`Home.tsx:186-197`), New factory and Work's Detail pane.
2. **Work out the onboarding step from engine state; don't store a parallel state machine.**
   - No factories (including unreadable ones), loaded without error → check and New factory.
   - A factory with no work → write a ticket.
   - Work at `approve` → approve.
   - Anything later → notes only.
   - Ended → done.

   At most one "dismissed" bit per machine is stored, in its own file in the state area, not in `settings.yaml` or `factory.yaml`.
3. **Key stage notes on stage kind and waiting value, not stage id.** Use stage kind (agent, gate, human) plus `Waiting` (`factories.rs:377-390`), not ids like `plan` or `verify`. List stages from `factory.line`. Ids can be renamed, and `verify` may be missing.
4. **Full notes only where the user acts.** Approve and handoff get full notes. Automated stages get one quiet line in the existing status text, with no colour or motion (PRODUCT.md:25,37).
5. **Show the trust statement before create, not before the first ticket.**
6. **Set first-run defaults instead of asking.** Folder only, with defaults for everything else and a line saying they can be changed later in setup. On first run, the reviewer is the same harness as the builder.
7. **Show hidden setup problems.** Problems from `app.apply` (`main.rs:384`) show on the first-run screen.
8. **Scope is stages 1–3 of the staging below, with no first-run mode.** Decided by Furkan on 2026-11-12.
   - The zoom-out lens proposed four small edits instead (a folder checklist in New factory, one sentence about worktrees, new Home empty copy, agent descriptions on running stages). Rejected as the whole scope: none of them fixes the four broken promises, and the builder of the app reports not knowing how to use it, which is evidence that the first run is hard, not only for strangers.
   - The zoom-out lens's other point is kept: every change is permanent and seen by every user. There is no "first run" flag that switches guidance on or off. The setup check, the trust statement and the per-state notes stay after the first ticket.
   - Stage 4 (fallback sample and measurement) is out of scope until one real new user has been watched.
   - PRODUCT.md's "Users" section says other engineers don't shape the first version. That is no longer true and should be updated as part of this work.

### Open questions that need your call

1. ~~Is this feature needed now?~~ Resolved: see decision 8.
2. **How to make the branch promise true.**
   - Options:
     - require git with one or more commits, and block create otherwise;
     - require `contained`, or reword the promise per level;
     - make a missing branch guard block the first ticket.
   - Also decide:
     - whether the factory's `pre-commit` hook should skip commits made outside the worktrees;
     - what happens if the user switches branch mid-ticket, given that merge targets whatever is checked out (`flow.rs:1174`).
3. **How "test command runs" is checked.**
   - Options: once, in a throwaway snapshot, the same way the gate runs it, with a time limit; or on the host; or not at all, just showing the command so it can be edited.
   - Also decide what happens when tests already fail on the base branch: refuse, run without the gate, or warn and allow.
   - And what happens with no test command: a warning that this line has no verify stage, or a prompt to type one.
4. **How "signed in" is checked.**
   - Options: a one-shot "reply ok" ping per harness, which needs `one_shot` to work without a saved factory and costs a model call; an optional auth command in harness profile data; or no check, relying on the terminal's "asking you" state.
   - Decide whether "can't tell" blocks.
5. **"Tell you when it needs you."**
   - Options: add OS notifications or a dock badge (a new plugin and permission prompt), or reword it to "it shows on Home under next up; stopcord doesn't notify outside the app".
   - Also decide what closing the window says the first time.
6. **Protected tests on a first ticket.** Either change the default line so tests a ticket adds don't raise a finding, or make this a planned, explained stop.
7. **Empty plan.** Either make the first-run planner always write a short plan, or say plainly what approving means when there is none.
8. **Where guidance ends and what "done" means.**
   - The concept says guidance ends at plan approval, but the first run ends at handoff. The structure lens proposes two named layers:
     - **guided:** blocking steps, through approve;
     - **annotated:** notes in place, from build to the end.
   - It also proposes "done" as the first ticket reaching any end: merged, sent back, dropped or failed.
   - Confirm this, and whether each note shows once or every time.
9. **The fallback.** Options: a practice clone of the user's own repo, a bundled sample, pasting a public repo URL, or none. If a sample is kept: where it lives, what its test command is, and how it is removed.
10. **Cost and time copy.** Should the app say before start how many agent sessions the line runs, on which harnesses, and roughly how long it takes?
11. **Measurement.** Local-only milestone entries in the run log (folder picked, checks passed, ticket written, approved, handoff, outcome), or nothing.
12. **Skipping onboarding.** How e2e tests skip it (an environment variable or a pre-seeded state file), and which data attributes the notes carry so tests can find them.

### Suggested staging

1. Make the four promises true, or reword them. Show hidden setup problems. Turn the setup check into a permanent "check this factory" feature in setup and machine settings, since sign-ins expire and tests break after the first run too.
2. Add the first-run defaults, the trust statement in New factory, and Home's empty-state copy.
3. Add state-specific notes in Work's Detail pane, covering the failure states as well as the happy path.
4. Decide the fallback and the measurement after watching one real new user. (Out of scope for this design.)
