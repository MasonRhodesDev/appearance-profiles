# appearance-profiles

A neutral, versioned standard for resolving desktop appearance across login
screens, lockscreens, compositors, theme engines, and settings applications.
It complements
[monitor-profiles](https://github.com/MasonRhodesDev/monitor-profiles): monitor
profiles describe topology; appearance profiles describe what is rendered.

## Layer contract

Implementations resolve each field independently, from highest to lowest:

1. runtime or command-line override;
2. user output override;
3. user global preference;
4. system output override;
5. system global default;
6. packaged output override;
7. packaged global default;
8. application fallback.

Canonical locations on Linux:

| Layer | Path |
|---|---|
| Packaged | `/usr/share/appearance-profiles/default.toml` |
| System | `/etc/appearance-profiles/default.toml` |
| User | `$XDG_CONFIG_HOME/appearance-profiles/default.toml` |
| Published user snapshot | `/var/lib/appearance-profiles/users/USER/default.toml` |

The published snapshot is a projection for pre-login consumers. Its assets
must be copied to the same user directory (or another greeter-readable cache);
it must not expose arbitrary paths inside a private or encrypted home.

## Version 1

```toml
version = 1

[background]
path = "wallpaper.png"
fit = "fill"

[output."desc:HPN HP E243 CNK7510Y4B"]
path = "portrait.png"
fit = "fill"
```

Output keys use the same connector and `desc:...` identities as
monitor-profiles. Relative asset paths resolve against the profile file.
Unknown fields are rejected. Missing or unreadable layers are skipped by
policy; malformed present layers are diagnostics, not silent defaults.

The Rust crate implements the normative merge and resolution algorithm.
`schema/v1.json` is the language-neutral validation contract.

## Prepared bundle service boundary

LMTT is the sole producer and lifecycle owner of prepared appearance bundles.
Greeters, lockers, compositors, and other UI processes are read-only consumers.
They must never require LMTT or a cache daemon to be running.

The producer atomically publishes `bundle.toml`, `tokens.json`, copied source
assets, and monitor-sized RGBA assets beneath:

```text
/var/lib/appearance-profiles/users/USER/
```

`bundle.toml` is versioned independently from the preference schema. Prepared
backgrounds are keyed by output selectors, pixel dimensions, and fit mode.
The preferred cache format is `xrgb8888-le`: four bytes per pixel in native
little-endian XRGB8888 memory order (`B, G, R, unused`), allowing software
scanout backends to copy the prepared frame without image decoding or scaling.
Consumers use an exact prepared match when available and retain a non-blocking
runtime fallback for cache misses, new monitor modes, or damaged assets.

Responsibilities are deliberately one-way:

| LMTT producer | Read-only consumers |
|---|---|
| Resolve and copy private source assets | Resolve the active profile |
| Decode, crop, scale, and encode prepared assets | Load an exact prepared asset |
| Publish tokens and manifests atomically | Fall back asynchronously on a miss |
| Version, invalidate, and garbage-collect generations | Never mutate the published bundle |

An optional cache warmer may call the same producer library, but it is not a
service dependency and must not sit on the login or lock critical path.
