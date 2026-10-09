Prebuilt macOS binaries for Apple Silicon and Intel (macOS 13 or later). SHA-256 checksums are included.

Homebrew:

```sh
brew install kitlangton/tap/modafinil
modafinil install
```

Closed-lid support is a separate, experimental opt-in requiring administrator approval:

```sh
modafinil set --closed-lid on
```

Default mode is AC-only. `modafinil set --mode always` keeps ordinary assertions on battery too; closed-lid support remains AC-only. Run `modafinil status` to inspect settings and services.

See the [README](https://github.com/kitlangton/modafinil#readme) for settings, removal, recovery and safety limits. Verify unplugging and actual closed-lid behavior on your own Mac. Never leave a running closed-lid laptop in a bag.

Version 0.1.1 fixes an asynchronous launchd-unload race: cleanup now waits for service removal, and unknown service-state reads fail rather than counting as absence.
