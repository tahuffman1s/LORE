---
description: Autonomously build the next roadmap issue end to end (research, plan, implement, PR, merge)
argument-hint: Optional issue number or task instead of the next roadmap issue
---

Build one roadmap issue to completion without asking the user anything.

Task: $ARGUMENTS. If empty, take the open issue labelled `roadmap` and not `blocked` in the earliest milestone (M0, then M1, up to M8), and within that milestone the lowest number. A follow-up filed in an early milestone has a high number and must not wait until last. This prints its number and title:
`gh issue list -l roadmap -S "-label:blocked" -L 300 --json number,title,milestone --jq 'map(select(.milestone)) | sort_by(.milestone.title, .number) | .[0] | "\(.number) \(.title)"'`
Then read it: `gh issue view <number> --json number,title,body`.

1. **Scope** — read the issue and only the code it touches. If it is too big for one session, split it into new `roadmap` issues in the same milestone and take the first.
2. **Branch** — `git switch main && git pull -q && git switch -c <number>-<short-slug>`.
3. **Research** — if there is a user-facing surface or a technique choice, send one focused question to the `researcher` agent first.
4. **Plan** — for multi-file work, get a design from the `code-architect` agent. Where there is a choice, pick the option that best serves the pillars in `CLAUDE.md`; do not ask. Record each non-obvious choice as one dated line in `docs/decisions.md`.
5. **Build** — implement in small steps with tests alongside. Look up crate APIs with context7.
6. **Verify** — run the check, clippy and test commands from `CLAUDE.md` and fix everything. Run `api-reviewer` if scripts or script-facing API changed, and `code-reviewer` on the diff; fix what they find. Run the thing for real when it can be run.
7. **PR** — commit, push the branch, `gh pr create` with a body following the template and `Closes #<number>`. Then `gh pr checks --watch`; fix failures until CI is green, and fix or answer every comment from the `claude[bot]` review (`gh pr view --json reviews,comments`).
8. **Merge** — `gh pr merge --squash --delete-branch`, then return to an up-to-date `main`. File any follow-ups you discovered as new issues.
9. **Report** — three lines at most: what now works, what was decided, what is blocked.

Blocked means only the user can act: a system package to install, credentials, a licensing question, or an irreversible action. Label the issue `blocked`, comment the reason in one line, and continue with the next unblocked issue instead of stopping.
