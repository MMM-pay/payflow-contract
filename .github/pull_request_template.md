## What this changes

<!-- One or two sentences. Link the issue: Closes #123 -->

## Why

<!-- The problem it solves, or the invariant it adds or keeps. -->

## How it was checked

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test --all` passes, and new behaviour has a test for the success path and for each error it can return
- [ ] `stellar contract build` succeeds
- [ ] New errors and events are documented, and existing error codes keep their numbers

## Notes for the reviewer

<!-- Anything you were unsure about, or follow-up work you left out. -->
