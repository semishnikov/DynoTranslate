# Workflow

How work moves through this repository: branches, commits, review, and what to do when a build
fails. Read this before touching anything; `docs/PLAN.md` says *what* to build, this says *how
it lands*.

## Branch model

- `main` is the release branch. It always builds and is never committed to directly.
- One unit of work is one branch, one pull request, one milestone. Milestones are listed in
  `docs/PLAN.md`; a milestone may be split across sessions, and that split is recorded in
  `docs/STATUS.md`.
- Work branches are created from `main` and carry the name the working environment assigns
  (`arena/<session-id>-dynotranslate`). The name is opaque: treat it as an ordinary feature
  branch and never rename or replace it mid-flight, because the session is tracked by it.
- Open the pull request as a draft as soon as the first commit lands, keep it updated, and mark
  it ready for review when the milestone is complete.
- All work stays on the branch of the current session. Never push to another session's branch.

## Commits

Conventional Commits, imperative mood, subject at most 72 characters, body explaining *why* when
the reason is not obvious from the diff.

    <type>(<scope>): <subject>

    <body: the constraint or decision that made this necessary>

Types in use: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `chore`, `build`. Scope is the
crate or area: `core`, `capture`, `source`, `ocr`, `overlay`, `cli`, `app`, `ci`, `status`.

    feat(source): add the UI Automation text source and the source merge
    fix(source): name the request lifetime and drop an unneeded mut
    docs(status): record M1 merge, M2 OCR slice verification

One logical change per commit. Never rewrite history that is already on the remote, and never
fabricate a history that did not happen.

## Pull requests

The title names the milestone slice. The body answers four questions: what changed, how it was
verified (with the CI run id and the real numbers), what could **not** be verified and why, and
what the owner must provide. Pull requests target `main`.

## Continuous integration

`.github/workflows/ci.yml` runs on pushes to `main` and on every pull request.

| Job | Platform | Steps |
| --- | --- | --- |
| `Interface` | `ubuntu-latest` | `npm ci`, `npm run lint`, `npm run build` in `app/` |
| `Rust (ubuntu-latest)` | Linux | fmt → clippy → test → pipeline run |
| `Rust (windows-latest)` | Windows | clippy → test → pipeline run |

The Rust jobs run, in order: install stable with `rustfmt` and `clippy`, restore the cargo cache,
`cargo fmt --all --check` (Ubuntu only), `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace --all-targets`, then the headless pipeline against a synthetic scene, and
finally upload the pipeline output as an artifact.

Three properties of this workflow are worth remembering:

1. `RUSTFLAGS: -D warnings` is set for every job, so **any** warning is a hard error. That
   includes `unused_mut`, `unused_imports`, `dead_code` and every clippy lint.
2. `fail-fast: false`. A failure on one platform does not cancel the other, so always look at
   both jobs; the Windows one often fails for reasons Linux cannot see.
3. Formatting is checked on Ubuntu only. On Ubuntu, `cargo fmt` runs *before* clippy, so a single
   formatting difference stops the job before the compiler ever runs and hides every compile
   error. When Ubuntu reports only a fmt diff, the Windows job is the one carrying the real
   compiler output.

### Reading a failure when the log is not reachable

The day-to-day development environment cannot open the Actions log host, and the check-run API
returns nothing but `Process completed with exit code 1`. For that reason each Rust job ends with
a `Report what failed` step, gated on `if: failure()`, which re-runs fmt and clippy and re-emits
what it finds as check annotations. Annotations are reachable through the ordinary API:

```sh
# the run and its jobs
gh run list --branch <branch> --limit 1
gh api repos/semishnikov/DynoTranslate/actions/runs/<run-id>/jobs \
  --jq '.jobs[] | "\(.id)\t\(.name)\t\(.conclusion)"'

# the diagnostics, 40 lines each for fmt and clippy
gh api "repos/semishnikov/DynoTranslate/check-runs/<job-id>/annotations?per_page=100" \
  --jq '.[] | "[\(.annotation_level)] \(.title) :: \(.message)"'
```

Annotation titles are `fmt` or `clippy`. This is the only channel available; do not plan around
`gh run view --log`, it does not resolve from the development environment.

If a new kind of failure needs the same treatment, extend that step rather than adding a job, and
keep the output bounded — annotations are capped and a full compiler transcript is not useful.

### Who may change the workflow

`.github/workflows/**` can only be committed by an account holding the `workflows` permission,
which is the repository owner. A push of a workflow change from any other account is rejected
outright. For `pull_request` events GitHub reads the workflow from the **head** commit, so a
change has to be committed to the work branch itself; editing `main` has no effect on an open
pull request. The owner applies it through the web editor, committing directly to the branch:

    https://github.com/semishnikov/DynoTranslate/edit/<branch>/.github/workflows/ci.yml

Anything else in the repository is ordinary to change.

## Local verification

There is no Rust toolchain in the development environment and no path to `crates.io` or
`static.rust-lang.org`, so nothing in the Rust workspace can be compiled, formatted or tested
locally. Consequences to plan for:

- Code is written against the published signatures of its dependencies and verified by CI. Read
  the dependency's real source before relying on an API; do not rely on memory.
- Formatting has to be predicted. `rustfmt.toml` sets `edition = "2021"` and `max_width = 120`;
  with the default heuristics that makes `fn_call_width` and `chain_width` 72 and
  `struct_lit_width` 21, which decides whether a construct stays on one line.
- Nothing is claimed to build, pass or be formatted until the workflow says so. A change that has
  been pushed but not yet green is described as pushed, not as done.

## Commands

```sh
git fetch origin                                  # what the remote has
git ls-remote origin refs/heads/<branch>          # the exact remote head, no stale refs
git status --short
git log --oneline -10
git add <explicit paths>                          # never -A: keep scratch files out of commits
git commit -m "type(scope): subject"
git pull --rebase origin <branch>                 # before pushing, when others may have pushed
git push origin <branch>

gh pr list --state open
gh pr view <number> --json state,isDraft,headRefName
gh pr checks <number>
gh run list --branch <branch>
gh run watch <run-id> --exit-status               # polls the API; exit 0 means every job passed
```

`gh run watch` and `gh run view` work, because they only talk to the API. `gh run view --log` does
not: it fetches from the log host.

Never `git push --force` on a branch another session or person may hold, never commit to `main`,
and never leave scratch files staged.

## Re-running a build

Pushing a commit is the normal trigger. When there is nothing to change, an empty commit is
acceptable and self-documenting:

```sh
git commit --allow-empty -m "chore(ci): re-run the workflow"
```

`concurrency` in the workflow cancels a run that a newer push to the same ref has superseded, so a
missing result usually means a newer commit arrived.

## Done means

CI green on all three jobs, lint clean, tests passing, `docs/STATUS.md` updated, and the artifacts
that back the claims (screenshots, benchmark numbers) committed under `docs/`. The next milestone
does not start on a red build.
