# Security policy

## Reporting a vulnerability

Please **do not open a public issue** for a security problem.

Report it privately through GitHub's
[security advisory form](https://github.com/pasichDev/chystik/security/advisories/new),
or by email to the address on the maintainer's GitHub profile.

Expect an acknowledgement within a week.

## What counts as a security issue here

Chystik deletes files, so its threat model is unusual. The following are
security issues, not ordinary bugs:

- **Guard bypass** — any path reaching `trash::delete` that
  `chystik_core::guard::check` should have refused: a protected prefix
  (`/`, `/boot`, `/etc`, `/usr`, `/var`, `/opt`, `/proc`, `/sys`, `/dev`), a
  protected name (`.git`, `.ssh`, `.gnupg`, `.config` outside its audited
  allowlist), anything outside the scan root, or a symlink.
- **Symlink traversal** — anything that makes the scanner or the deletion path
  follow a link out of the scan root, including a link swapped between the
  guard check and the delete.
- **A rule matching user data** — a rule that classifies documents, source
  code, credentials or configuration as reclaimable. Include the path pattern
  and what it hit.
- **Privilege issues** — Chystik is a normal user-level application. It must
  never require or request root, and must never be able to modify anything
  outside the invoking user's reach.

## Not security issues

- A rule mis-rating severity (Safe where Moderate was warranted) — a normal
  bug; open an issue.
- Chystik missing something it could have found — a feature request.
- Bugs in a dependency that Chystik does not expose. Report those upstream.

## Scope of the guarantee

Deletion is trash-only: everything goes through the XDG trash and is
restorable from your file manager. Chystik never calls `remove_dir_all` or
`unlink` on a user path. A report showing otherwise is a valid vulnerability.
