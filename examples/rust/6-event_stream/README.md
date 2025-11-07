# Event Stream

Subscribe to Server-Sent Events (SSE) from a Pubky user's homeserver `/events-stream` endpoint.

## Usage

```bash
cargo run --bin event_stream -- <USER_PUBKEY> [OPTIONS]
```

### Options

- `<USER_PUBKEY>`: User's public key in z32 format
- `-l, --limit <N>`: Maximum number of events to fetch
- `-L, --live`: Enable live streaming mode (receive new events as they occur)
- `-r, --reverse`: Reverse chronological order (newest first)
- `-p, --path <PATH>`: Filter events by path prefix (e.g., `/pub/posts/`)
- `-c, --cursor <CURSOR>`: Start from this cursor
- `--testnet`: Use local testnet endpoints

## Examples

### Get last 10 events

```bash
cargo run --bin event_stream -- o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo --limit 10
```

### Live stream new events

```bash
cargo run --bin event_stream -- o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo --live
```

### Filter by path in reverse order

```bash
cargo run --bin event_stream -- o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo \
  --path "/pub/posts/" \
  --reverse \
  --limit 20
```

### Start from a specific cursor

```bash
cargo run --bin event_stream -- o1gg96ewuojmopcjbz8895478wdtxtzzuxnfjjz8o8e77csa1ngo \
  --cursor "2024-01-15T10:30:00Z" \
  --limit 50
```

### Use with testnet

```bash
cargo run --bin event_stream -- 8pinxxgqs41n4aididenw5apqp1urfmzdztr8jt4abrkdn435ewo \
  --testnet \
  --live
```

## Output Format

Each event is printed as:

```
[PUT|DEL] <path> (cursor: <timestamp>, hash: <content_hash>)
```

Example:
```
[PUT] /pub/posts/123 (cursor: 2024-01-15T10:30:00Z, hash: abc123...)
[DEL] /pub/posts/456 (cursor: 2024-01-15T10:31:00Z, hash: -)
```
