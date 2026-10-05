# Removed documents

> **Status:** Current. Append a row for every document that you delete.
> **Summary:** Documents deleted as stale or redundant, with the reason and the command that restores each one.

Git history keeps every removed file. To read one, run `git show <commit>:<path>`.
The commit column is the last commit that contained the file.

| Path | Commit | Reason |
|---|---|---|
| `docs/reviews/2026-07-02-comprehensive-code-review.md` | `a0eadae15a31` | All findings (F1 to F7, MT1 to MT4) are fixed in 0.2.0 and 0.3.0. [worklog/2026-07-03.md](worklog/2026-07-03.md) records each defect, fix, and test. |
| `docs/worklog/2026-07-04.md` | `a0eadae15a31` | 0.2.0 release summary that repeats CHANGELOG.md and the 2026-07-03 worklog. Its F3 deferral is stale: F3 shipped in 0.3.0. |
