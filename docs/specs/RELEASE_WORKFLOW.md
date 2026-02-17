# Release Workflow & Versioning Policy

Status: Canonical for compiler-mode release operations (M4 in locked sequence M5 -> M1 -> M2 -> M3 -> M4)

This document defines how `tsgodown` versions, cuts releases, and handles rollback/hotfix scenarios.

## 1) Versioning Strategy

`tsgodown` uses **Semantic Versioning (SemVer)** for all publishable packages (`major.minor.patch`).

- **MAJOR**: breaking API/behavior changes
- **MINOR**: backward-compatible features
- **PATCH**: backward-compatible fixes

### Workspace version policy

- Keep workspace packages aligned on one release version for predictable cross-package compatibility.
- Current baseline is `0.y.z` (pre-1.0):
  - Breaking changes may occur in minor bumps; still document in changelog.
  - Patch bumps remain strictly bug-fix only.
- Do not version-bump deprecated/inactive package `@tsgodown/ir` for release signaling.

### Branching model

- `main`: releasable branch
- `feat/*`: feature work
- `fix/*`: non-urgent bug fix
- `hotfix/*`: urgent production fix, branched from latest release tag

## 2) Release Checklist

Run from repo root on a clean tree.

### A. Prepare

1. Sync branch:

```bash
git checkout main
git pull --ff-only origin main
```

2. Ensure no local changes:

```bash
git status --short
```

3. Install exact dependencies:

```bash
pnpm install --frozen-lockfile
```

### B. Quality gates (required)

```bash
pnpm run lint
pnpm run format:check
pnpm run build
pnpm run test
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
./scripts/smoke-m1.sh
```

### C. Bump versions

Example: patch release (`0.0.1` -> `0.0.2`) for all active packages.

```bash
# root
pnpm version 0.0.2 --no-git-tag-version

# each active package
for f in packages/*/package.json; do
  if ! grep -q '"name": "@tsgodown/ir"' "$f"; then
    (cd "$(dirname "$f")" && pnpm version 0.0.2 --no-git-tag-version)
  fi
done
```

Re-run build/tests after bump:

```bash
pnpm run build
pnpm run test
```

### D. Changelog + tag + release commit

```bash
git add -A
git commit -m "chore(release): v0.0.2"
git tag -a v0.0.2 -m "Release v0.0.2"
git push origin main --follow-tags
```

## 3) Rollback Runbook

Use when a newly released tag is bad.

### Scenario A: rollback release pointer (preferred, no history rewrite)

1. Revert problematic release commit(s):

```bash
git checkout main
git pull --ff-only origin main
git revert <bad_commit_sha>
git push origin main
```

2. Cut a corrective patch release (`vX.Y.(Z+1)`) using the checklist.

### Scenario B: operational rollback to previous stable tag

If consumers need an immediate known-good reference:

```bash
# inspect tags
git tag --sort=-v:refname | head

# checkout prior stable for local verification
git checkout v0.0.1
pnpm run build && pnpm run test
```

Communicate: "temporary rollback target is `v0.0.1`; fix-forward patch incoming."

## 4) Hotfix Runbook

For urgent production defects.

1. Branch from latest stable tag:

```bash
git fetch --tags origin
git checkout -b hotfix/critical-crash v0.0.2
```

2. Implement minimal fix + tests.

3. Run full required gates (same as release checklist).

4. Bump patch version only (e.g., `0.0.2 -> 0.0.3`), then:

```bash
git add -A
git commit -m "fix(hotfix): prevent critical crash"
git tag -a v0.0.3 -m "Hotfix v0.0.3"
git push origin hotfix/critical-crash
git push origin v0.0.3
```

5. Merge hotfix back to `main` immediately:

```bash
git checkout main
git pull --ff-only origin main
git merge --ff-only hotfix/critical-crash || git merge --no-ff hotfix/critical-crash
git push origin main
```

## 5) PR Requirements for Release/Hotfix

- PR title prefix: `release:` or `hotfix:`
- Must include:
  - target version
  - change summary
  - risk/impact
  - rollback plan
  - proof of full gate command results
- Link issue(s) and milestone stage explicitly (example: `Closes #<id>`, `Milestone: M4 (sequence M5->M1->M2->M3->M4)`).
