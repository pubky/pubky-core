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

1. Merge all PRs in the main branch that you want to include in the next version.
2. Update versions of all crates and npm package with `./.script/set-version.sh $NEW_SEMVER_VERSION`.
4. Create a PR with the title: `chore: vx.x.x`.
5. Let the PR review and squash + merge.
6. Publish crates and npm package.
  - Checkout the `main` branch with the new version merged.
  - Run `./.script/publish-libs.sh`.
7. Create a [new Github release](https://github.com/pubky/pubky-core/releases/new).
    - Tag: `vx.x.x`
    - Title: `vx.x.x`
    - Description: Changelog for the current version.
    - Upload the different artifacts created by the [build-artifacts.yml workflow](./.github/workflows/build-artifacts.yml). 
    You can find them in [Github Actions](https://github.com/pubky/pubky-core/actions?query=branch%3Amain) for the new main commit.
