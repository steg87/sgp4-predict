# Releasing

Releases run entirely in GitHub Actions. Nothing is published from a laptop, and
no local script needs to be run.

Two workflows do the work:

| Workflow | Trigger | Does |
|---|---|---|
| **Prepare release** (`release-prepare.yml`) | manual dispatch | bumps versions, rolls changelogs, verifies, opens a release PR |
| **Release** (`release.yml`) | push to a release branch | publishes to crates.io + PyPI, tags, opens GitHub Releases |

The bump and changelog roll are driven by [`cargo-release`](https://github.com/crate-ci/cargo-release);
its configuration lives in [`release.toml`](../release.toml).

## Versioning

All three crates share **major.minor**; **patch** moves independently.

- `sgp4-predict` and `sgp4-predict-cli` publish to crates.io.
- `sgp4-predict-py` publishes to PyPI as `sgp4-predict` (it is `publish = false`
  for cargo). Its Cargo version is the single source of truth — maturin reads it,
  so `pyproject.toml` declares `dynamic = ["version"]`.

A `minor` or `major` bump is workspace-wide and zeroes patch everywhere, so the
crates re-align automatically. A `patch` bump may be scoped to one crate — that
is how a cli-only fix ships without dragging the library along.

The invariant is enforced twice: **Prepare release** refuses to open a PR that
would break it, and `test.yml`'s `versions` job asserts it on every PR.

## Making a release

1. **Write notes as you go.** Every PR that changes behaviour adds bullets under
   `## [Unreleased]` in the affected crate's `CHANGELOG.md`.

2. **Dispatch _Prepare release_.** Actions → *Prepare release* → *Run workflow*:

   | Input | Meaning |
   |---|---|
   | `branch` | branch to release from (default `main`) |
   | `level` | `patch` / `minor` / `major` / `rc` / `beta` / `alpha` / `release` |
   | `version` | an exact version, overriding `level` — e.g. `0.2.0-rc.1` |
   | `scope` | `all`, or a single crate (patch bumps only) |

3. **Review the PR.** It shows the version table, the changelog diff, and a
   *Release notes preview* — the exact text each GitHub Release will carry. The
   normal test suite runs on it. Amend the changelog wording on the PR branch if
   you want; notes are re-extracted from the merged files at publish time.

4. **Merge it.** *Release* then publishes every crate whose
   `<name>-v<version>` tag does not yet exist, tags it, and opens a GitHub
   Release per crate.

### Pre-releases

`rc`, `beta` and `alpha` bump into a pre-release series:

```
0.1.0        --level rc--> 0.1.1-rc.1  --level rc--> 0.1.1-rc.2
0.1.1-rc.2   --level release--> 0.1.1
```

To cut a release candidate for the *next minor* rather than the next patch, pass
the exact version: `version = 0.2.0-rc.1`.

Pre-releases **do not roll the changelog**. A release candidate ships the pending
notes as they stand, so they stay under `## [Unreleased]` and are published again
with the final release. `extract-changelog.sh` reads `[Unreleased]` for any
version carrying a pre-release suffix.

### Maintenance branches

`release.yml` also triggers on `*.x`, so a backport patch can ship from an older
line — releasing `1.1.2` after `1.2.0` is already out. Branch from the tag as
`1.1.x`, cherry-pick, then dispatch *Prepare release* with `branch = 1.1.x`.

**Name maintenance branches `<major>.<minor>.x`, never `release/…`.** `release/`
is the namespace *Prepare release* opens its PR branches in, so `release.yml`
deliberately does not trigger on it. Adding `release/**` to those triggers would
publish a release PR branch the moment it was pushed, before anyone reviewed it.

## Repository setup

Required once:

- A **`release` label**, which *Prepare release* attaches to the PR it opens.
- **`cargo.io` environment** with a `CARGO_TOKEN` secret (a crates.io API
  token). The environment name must match `release.yml` exactly — a mismatch
  makes GitHub create an empty environment, and `cargo publish` then fails
  authentication with no obvious cause.
- **`pypi` environment** — no secret; PyPI uses OIDC trusted publishing.
  Configure a publisher on PyPI for `sgp4-predict` with owner `steg87`,
  repository `sgp4-predict`, workflow `release.yml`, environment `pypi`. For a
  project that does not exist on PyPI yet this is a *pending* publisher, and it
  must exist **before** the first publish.
- Adding **required reviewers** to either environment makes publishing pause for
  an explicit approval even after the release PR merges.

### A token that lets CI run on the release PR

GitHub suppresses events created by `GITHUB_TOKEN` — this is deliberate, to stop
a workflow that pushes from re-triggering itself forever. The consequence is that
a PR opened with `GITHUB_TOKEN` never starts `test.yml`.

A GitHub App fixes it, and is what this repo uses:

| Setting | Value |
|---|---|
| `RELEASE_APP_CLIENT_ID` **variable** | the App's Client ID |
| `RELEASE_APP_PRIVATE_KEY` **secret** | the full `.pem`, `BEGIN`/`END` lines included |

To set it up: create the App with **contents: write** and **pull requests:
write**, copy its **Client ID** (not the App ID — both are on the App's settings
page), *Generate a private key*, and **install the App on this repository**.
Creating it is not enough; without the install the token request fails with
`Not Found`.

The private key is how App auth works: the action signs a JWT with it and
exchanges that for a short-lived installation token. A client secret cannot be
used — that belongs to the OAuth user flow, which needs a browser and a human.

There is deliberately no fallback: if `RELEASE_APP_CLIENT_ID` is unset,
*Prepare release* fails immediately rather than opening a PR that silently gets
no CI.

Note the App cannot approve its own PR, so a required-review rule on the
base branch still needs a human — which is the intent for a release.

## Recovering from a partial release

Tag absence is the release signal, so re-running *Release* resumes rather than
duplicating:

- Published but not tagged? `cargo info <name>@<version>` detects it, the publish
  is skipped, and only the tag and GitHub Release are created.
- Tagged already? That crate is not pending and is skipped entirely.

To rehearse without side effects, dispatch *Release* with `dry_run: true`
(the default): detection and the wheel builds run, publishing and tagging do not.

## First release

There are no tags yet and all three crates sit at `0.1.0` with a written
`## [0.1.0]` section, so **v0.1.0 needs no _Prepare release_ run** — merging to
`main` makes *Release* see three untagged versions and publish all three.

Rehearse it first with a `dry_run: true` dispatch. To defer instead, push the
three `*-v0.1.0` tags by hand to suppress detection.
