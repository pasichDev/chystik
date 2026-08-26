## Summary

Describe the user-visible and safety-relevant result.

## Validation

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Cleanup rule safety checklist

Complete this section when changing catalog/rule data.

- [ ] The exact path was verified; no broad parent, profile, project root, or application-data directory is proposed.
- [ ] Nearby sibling paths were checked and have a negative test where needed.
- [ ] An authoritative HTTPS vendor/upstream source URL is included.
- [ ] Recovery behavior was verified and is distinct from cleanup policy.
- [ ] Cleanup policy is justified; `Auto-cleanable` is both automatic recovery and guard-compatible.
- [ ] Fixtures/tests pass and no user-specific or private path is included.

## Risks / rollback

State known limitations and how to revert safely.
