# Working in this repository

Notes a session cannot derive from the code, and that have each already cost
one. Everything else — structure, conventions, the reasoning behind a design —
lives in `docs/design/`, and `docs/design/README.md` is the way in.

## Branches and merges

**Always open pull requests against the default branch (`main`).** Not against
another PR's branch. `main` squash-merges, so a stack is structurally hostile:
the parent's commits are rewritten into one new commit, and the child still
carries the originals.

> **When a parent squash-merges, do _not_ rebase the child. Re-create the child
> from the default branch and cherry-pick only the child's own commits.**

Rebasing replays commits that add files the squash already added, so every one
of them collides `add/add`. Parallax #44 carried 32 such commits and was
recovered exactly the way the rule says — one cherry-pick, diff matched.

`delete_branch_on_merge` is on, which fixes *retargeting* — a child whose parent
merges is repointed at `main` rather than left aimed at a deleted branch. It
does **not** fix the squash collision; no setting does. The two are independent
problems and only the first has a switch.

Keep a follow-on branch to **one commit** where you can. The recovery above is
cheap at one commit and tedious at thirty.

`.github/workflows/branch-hygiene.yml` enforces the targeting half on every PR.

## CI

`gate` is the single required status check. It is an aggregator: `if: always()`
over `needs: [build, test, fmt, clippy]`. Require that name and no other —
matrix job names are generated, so requiring them directly rots when the matrix
changes.

There is **no docs-only skip**, deliberately. See the header of
`.github/workflows/ci.yml` for why; the short version is that `**/*.md` exempted
a file that `include_str!` compiles into the binary.

**`strict` is on as of 2026-08-21: a PR must be up to date with `main` before it
merges.** So when `main` moves under an open PR, `gate` passing is no longer
enough — update the branch and let it re-run:

```
gh pr update-branch <n>
```

This is not theoretical. #56 was green against a base without slice 4 while both
touched `series_from`, and the branch was updated by hand before merging. That
should not depend on someone noticing. The cost is one command per PR when the
base moves; the alternative is a green check that verified a combination nobody
ever built.

## Diagnosing branch protection

**Check both endpoints. Neither one alone means "unprotected".**

- `GET /repos/{owner}/{repo}/branches/{branch}/protection` — classic protection.
  Returns **404** when a branch is protected by a *ruleset*.
- `GET /repos/{owner}/{repo}/rulesets` — rulesets. Returns **`[]`** when a branch
  is protected by *classic* protection, which is this repo's case.

Reading one and concluding from its empty answer has produced a wrong call three
times across these repos, in both directions. `main` here is protected by
classic branch protection: required check `gate`, force-pushes and deletions
blocked, `enforce_admins` deliberately **off** so the owner can still intervene
by hand. Do not turn `enforce_admins` on without asking.
