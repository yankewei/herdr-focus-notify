---
name: release
description: Cut a new herdr-focus-notify release - bump the version in Cargo.toml, herdr-plugin.toml and Cargo.lock, date the CHANGELOG, run the CI checks, land the release commit on main through a pull request, tag it, and publish a GitHub Release. Use when the user asks to release, publish, ship, tag, or cut a new version of the plugin (e.g. "发一个新版本", "release v0.8.0").
---

# Release herdr-focus-notify

Each release is a single `chore: release vX.Y.Z` commit on `main`, an annotated tag `vX.Y.Z` on that commit, and a GitHub Release with no binary assets: Herdr builds the plugin itself from the tag using `herdr-plugin.toml`.

`main` is protected: changes must go through a pull request, and the `check` status (CI) must pass. Direct pushes to `main` are rejected with `GH013`, so the release commit lands through a `release/vX.Y.Z` pull request, and the tag goes on the commit that the merge puts on `main`.

Opening and merging the release PR, pushing a tag, and publishing a release are outward-facing. Do them only when the user has asked for a release in this conversation.

## 1. Preflight

```sh
git switch main && git pull --ff-only && git fetch --tags
git status --porcelain            # must be empty
git describe --tags --abbrev=0    # previous release tag
git log --oneline <prev-tag>..HEAD
```

Stop and report if:
- the working tree is dirty
- there are no commits since the previous tag
- `CHANGELOG.md` has no `## [Unreleased]` section, or that section does not describe the commits since the previous tag

Fix the changelog first, with the user's agreement, if it is missing entries.

## 2. Choose the version

Pick the bump from the `[Unreleased]` entries:
- **New feature** (anything under `### Added`): bump the minor version and reset the patch, e.g. `0.6.0` → `0.7.0`, `0.5.2` → `0.6.0`.
- **Bug fixes only** (`### Fixed`): bump the patch version, e.g. `0.6.0` → `0.6.1`.

State the chosen version in one line and proceed. Ask if the user named a different version, or if the entries are neither clearly a feature nor a fix (for example, only `### Changed`). The new tag must not already exist (`git ls-remote --tags origin vX.Y.Z`).

## 3. Bump and date

Work on a release branch:

```sh
git switch -c release/vNEW
```

Three version fields must stay aligned:

```sh
sed -i '' 's/^version = "OLD"/version = "NEW"/' Cargo.toml herdr-plugin.toml
```

`Cargo.lock` updates on the next cargo build. In `CHANGELOG.md`, rename `## [Unreleased]` to `## [NEW] - YYYY-MM-DD`, using today's date. Do not leave an empty `[Unreleased]` heading behind; earlier releases don't keep one.

Check `README.md` / `README.zh-CN.md` for version-specific guidance (for example the "use plugin tag vX or later" note near the top). Update it only if this release changes that guidance.

## 4. Verify

Run the same checks as CI (`.github/workflows/ci.yml`), in order:

```sh
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
target/release/herdr-focus-notify --version   # must print the new version
git diff --stat   # exactly CHANGELOG.md, Cargo.lock, Cargo.toml, herdr-plugin.toml
```

Stop on any failure.

## 5. Commit and open the release PR

```sh
git commit -am "chore: release vNEW

Bump Cargo, plugin manifest, and lockfile to NEW and date the NEW
changes: <one-sentence summary of the headline change>.

<attribution trailer from the session, if any>"
git push -u origin release/vNEW
gh pr create --base main --head release/vNEW --title "chore: release vNEW" --body "<the new CHANGELOG section, plus the session's PR attribution line, if any>"
```

Do not tag yet: the merge creates a new commit on `main`, and the tag belongs on that one.

## 6. Merge and tag

Wait for CI, then squash-merge so `main` gets exactly one `chore: release vNEW` commit:

```sh
gh pr checks <PR> --watch
gh pr merge <PR> --squash --delete-branch
git switch main && git pull --ff-only
git log -1 --format=%s    # must be "chore: release vNEW"
git tag -a vNEW -m "Release vNEW"
git push origin vNEW
git branch -D release/vNEW   # if it is still around locally
```

Stop and report if CI fails, or if the merge is blocked. The tag must be annotated, with exactly the message `Release vNEW`, and must point at the release commit on `main`.

## 7. GitHub Release

Write the notes to a scratch file, then run `gh release create vNEW --title vNEW --notes-file <file> --verify-tag`. Use this format:

```markdown
## Highlights

- <user-facing change, rewritten for readers of the release page, not copied from the CHANGELOG>
- ...

Full details in [CHANGELOG.md](https://github.com/yankewei/herdr-focus-notify/blob/vNEW/CHANGELOG.md). Closes #N.

## Validation

- `cargo fmt -- --check`
- `cargo test`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --release`
- <manual verification>
```

- Include `Closes #N` only for issues this release resolves.
- Only list manual verification that someone actually did and confirmed in the conversation, such as a real notification click in a named terminal. Never infer manual testing from passing unit tests. If nothing was verified by hand, leave the line out.

## 8. Confirm

```sh
gh release list -L 2      # new release is Latest
gh run list --branch main -L 1
```

Report to the user:
- the release URL, the release PR, and the release commit
- the CI run's status: still running, passed, or failed, with a link

Do not describe a CI run that is still in progress as passing.
