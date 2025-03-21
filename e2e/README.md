# Pubky End2End Tests

This workspace member contains Pubky End2End tests. Run them with `cargo test`.

## Pubky Testing Strategy


### Unit Testing

Each member of this workspace, for example `pkarr-republisher`, are tested individually
in their respective member folder. Dependencies like the `pubky-homeserver`
should be mocked. Focus on testing the individual components and not all of Pubky.

### E2E Testing

E2E tests cover multiple workspace members. Test full workflows. 
It's recommended to use `pubky-testnet` which provides a convinient way to run Pubky components.