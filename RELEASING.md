# Releasing pubky-core

## Prerequisites

- Push access to the repository (to create tags)
- `CARGO_REGISTRY_TOKEN` secret configured in GitHub repo settings
  (Settings > Secrets and variables > Actions > New repository secret)
- `NPM_TOKEN` secret configured for npm publishing

## Release Steps

### 1. Bump versions and create version PR

Make sure you are working on the most recent main branch.

```bash
git checkout main
git pull origin main
```

Run the version bump script:

```bash
./.scripts/set-version.sh 0.8.0
```

This updates all workspace crate versions and the npm `package.json`.

Create the version PR:

```bash
git checkout -b chore/v0.8.0
git add -A
git commit -m "chore: v0.8.0"
git push origin chore/v0.8.0
```

PR title: `chore: v0.8.0`

### 2. Tag and push

After the version PR has been reviewed and squash-merged:

```bash
git checkout main
git pull origin main
git tag v0.8.0
git push origin v0.8.0
```

This triggers the release workflow. That's it -- CI handles the rest.

## What CI Does Automatically

When a `v*` tag is pushed, the [release workflow](.github/workflows/release.yml):

1. **Validates** the tag is on `main`, is valid semver, and matches the version in `Cargo.toml`
2. **Builds artifacts** for all platforms (linux-arm64, linux-amd64, windows-amd64, osx-arm64, osx-amd64)
3. **Creates a GitHub Release** with auto-generated release notes and attached artifacts
4. **Publishes all crates to crates.io** via `cargo ws publish`
5. **Publishes the npm package** to npmjs.com

## Post-Release Verification

- [ ] [GitHub Releases](https://github.com/pubky/pubky-core/releases) -- new release with artifacts attached
- [ ] [crates.io](https://crates.io/crates/pubky-sdk) -- new version visible
- [ ] [npmjs.com](https://www.npmjs.com/package/pubky) -- new version visible

## Versioning

See [CONTRIBUTORS.md](CONTRIBUTORS.md#versioning) for the unified version policy. All crates are released together on the same version.

Follow [Semantic Versioning](https://semver.org/):

- **Patch** (0.7.x): bug fixes, no API changes
- **Minor** (0.x.0): new features, backwards-compatible
- **Major** (x.0.0): breaking API changes

## Troubleshooting

**CI build fails?** Fix the issue, then delete and re-push the tag:

```bash
git tag -d v0.8.0
git push origin :refs/tags/v0.8.0
# fix the issue, commit, push to main
git tag v0.8.0
git push origin v0.8.0
```

**cargo publish fails?** Check that `CARGO_REGISTRY_TOKEN` is set and not expired. Generate a new token at [crates.io/settings/tokens](https://crates.io/settings/tokens).

**npm publish fails?** Check that `NPM_TOKEN` is set and not expired. Generate a new token in your [npmjs.com account settings](https://www.npmjs.com/settings/~/tokens).
