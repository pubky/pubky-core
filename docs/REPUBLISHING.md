# Homeserver PKARR Republishing

This covers one `Republisher::republish` attempt together with the retry loop in
`RetryingRepublisher`.

```mermaid
stateDiagram-v2
    state "Try cached packet (CacheOnly)" as Cached
    state "Resolve network and choose latest (NetworkOnly)" as Latest
    state "Retry?" as Retry
    state "Published" as Published
    state "Skipped" as Skipped
    state "Missing" as Missing
    state "Invalid signed packet" as Invalid
    state "Failed" as Failed

    [*] --> Cached

    Cached --> Published: condition met and published sufficiently
    Cached --> Latest: miss, invalid, lookup failure, condition false, or NotMostRecent

    note right of Cached
        Cache is queried once per attempt;
        when present, its snapshot is reused by Latest
    end note

    Latest --> Published: latest valid packet accepted and published sufficiently
    Latest --> Skipped: condition false
    Latest --> Missing: no usable packet
    Latest --> Invalid: invalid signed packet newer than cache, or no cache
    Latest --> Retry: other error

    Retry --> Cached: attempts remain, after backoff
    Retry --> Failed: attempts exhausted

    Published --> [*]
    Skipped --> [*]
    Missing --> [*]
    Invalid --> [*]
    Failed --> [*]
```

"Invalid signed packet" means the DHT mutable item is valid, but its payload is
not a valid Pkarr signed packet. An equal or older invalid sequence is covered
by the cached packet and continues through `Latest`.

Known limitation: a cached publish can report success while a newer packet
exists on a minority of queried nodes. See
[mainline#113](https://github.com/pubky/mainline/issues/113).
