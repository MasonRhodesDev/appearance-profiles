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

