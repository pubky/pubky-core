# Contributors

## Create a Feature

1. (Optional) Describe the problem that the PR is solving in an issue first.
2. Fork the repo and create your feature branch. 
    -  Avoid having multiple features in one branch. See [Separation of Concerns](https://nalexn.github.io/separation-of-concerns/).
    -  Do not create feature branches in the main repository.
3. Code your feature.
    - Commits do NOT need to follow any convention.
4. Create a PR when finished. Use [Conventional Commits format](https://www.conventionalcommits.org/) as the PR title.
    - PR title format: `type (module): Summary of the changes`. Possible types are:
        - `BREAKING CHANGE` For changes that break the API.
        - `feat` For new features.
        - `fix` For bug fixes
        - `chore` For everything that is not covered in the above types.
        - The module is the workspace member name, for example `homeserver`, `client`, or `testnet`.
    - Use the [Draft feature](https://github.blog/2019-02-14-introducing-draft-pull-requests/) in case you need an early review.
    - Assign a reviewer. Every PR needs to be reviewed at least once. More reviews are possible on request.
5. Always squash the PR when merging. One commit == one feature/fix.

## Versioning

### Unified Version Policy

The following crates are released together on the same version schedule:

- `pubky-sdk`
- `pubky-homeserver`
- `pubky-testnet`
- `pubky-common`

**All four crates always share the same version number.** When any of these crates is released, all are released together with the same version.

This policy exists for **clarity and compatibility guarantees**:

1. **Compatibility assurance**: When `pubky-sdk` and `pubky-homeserver` share the same version (e.g., both at `0.6.0`), users know they are compatible and jointly tested. There's no guesswork about which SDK version works with which homeserver.

2. **Testing clarity**: As a developer, your production code uses `pubky::Client` and your tests use `pubky_testnet::Testnet`. When both are at `0.7.0`, you know your tests exercise the exact same client behavior you'll get in production.

### What If Only One Crate Needs Changes?

If a change affects only one crate (e.g., a testnet-only feature), we still release all crates together:

- **Minor changes**: Wait to bundle with other changes, or release all crates with the patch bump.
- **Urgent changes**: Consider a pre-release version like `0.6.1-rc.1` if you absolutely cannot wait. This signals "this is not a full release" while keeping versions aligned.

**Do not** release crates independently. The short-term convenience is not worth the long-term confusion it causes for users trying to match compatible versions.

### Release Process

1. Merge all PRs in the main branch that you want to include in the next version.
2. Update versions of all crates and npm package with `./.scripts/set-version.sh $NEW_SEMVER_VERSION`.
3. Create a PR with the title: `chore: vx.x.x`.
4. Let the PR review and squash + merge.
5. Publish crates and npm package.
  - Checkout the `main` branch with the new version merged.
  - Run `./.scripts/publish-libs.sh`.
6. Create a [new Github release](https://github.com/pubky/pubky-core/releases/new).
    - Tag: `vx.x.x`
    - Title: `vx.x.x`
    - Description: Changelog for the current version.
    - Upload the different artifacts created by the [build-artifacts.yml workflow](./.github/workflows/build-artifacts.yml).
    You can find them in [Github Actions](https://github.com/pubky/pubky-core/actions?query=branch%3Amain) for the new main commit.
