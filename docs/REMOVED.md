# Removed documents

> **Status:** Current. Append a row for every document that you delete.
> **Summary:** Documents deleted as stale or redundant, with the reason and the command that restores each one.

Git history keeps every removed file. To read one, run `git show <commit>:<path>`.
The commit column is the last commit that contained the file.

| Path | Commit | Reason |
|---|---|---|
| `docs/reviews/2026-07-02-comprehensive-code-review.md` | `a0eadae15a31` | Findings F1, F2, F4 to F7, and MT2 to MT4 are fixed in 0.2.0, and F3 in 0.3.0 (see CHANGELOG.md). [worklog/2026-07-03.md](worklog/2026-07-03.md) records each 0.2.0 defect, fix, and test. Open items from it are listed in [README.md](README.md#open-items). |
| `docs/worklog/2026-07-04.md` | `a0eadae15a31` | 0.2.0 release summary that repeats CHANGELOG.md and the 2026-07-03 worklog. Its F3 deferral is stale: F3 shipped in 0.3.0. |
