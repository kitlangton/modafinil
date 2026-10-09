# Modafinil CLI

A macOS keep-awake tool for long-running tasks. By default, it keeps your Mac awake while plugged in and releases its assertion on battery. The display can still turn off.

This is an independent Rust CLI, unrelated to the existing Modafinil menu-bar apps. It has no menu-bar UI.

## Install

Requires macOS and [Rust](https://rustup.rs/). Ordinary keep-awake needs no administrator permission.

```sh
cargo install modafinil-cli --locked
modafinil install
modafinil status
```

`install` copies the binary into your user directory, starts a launch agent now, and starts it at login. It does not enable closed-lid support or modify your power settings. To run only in a terminal:

```sh
modafinil run
# Ctrl-C stops it.
```

## Settings

```sh
modafinil set --mode ac       # Default: awake while plugged in.
modafinil set --mode always   # Awake on battery too; lid remains subject to normal sleep.
modafinil set --mode off      # Release assertions and remove the lid helper, if installed.
```

The login agent reads saved settings every two seconds. In every mode, an unreadable or unrecognized power source releases this tool's ordinary assertion. Other apps may still prevent sleep.

For a temporary override, use `modafinil run --mode always`; stop the login service first if it is running. Overrides do not change the separately installed closed-lid helper.

```sh
modafinil stop               # Stop the login service; its plist remains for next login.
modafinil start              # Resume the installed login service.
modafinil run --mode ac --seconds 10
```

### Closed lid (experimental)

Ordinary `caffeinate` assertions do not defeat standalone MacBook lid sleep. Optional closed-lid support uses the system-wide `pmset disablesleep` setting and requires administrator approval. It only activates on AC power, including when the ordinary mode is `always`.

Install ordinary mode first, then opt in:

```sh
modafinil install
modafinil set --closed-lid on  # sudo password prompt; installs a root-owned launch daemon.
modafinil status
```

To remove the privileged helper and restore system sleep:

```sh
modafinil set --closed-lid off
```

The helper starts at boot, independently of your login agent. It reads the power source every two seconds and clears `disablesleep` on battery, unknown power, check failures, and graceful exit. Failed restoration is logged and retried. If it crashes, launchd restarts it and startup clears the flag before reading power. This is recovery, not a guarantee: a force kill, launchd failure, hung system command or OS failure can leave a persistent setting. Unplugging can take a few seconds to restore sleep, longer if commands time out.

Do not run it alongside another app that changes `disablesleep`. The initial opt-in refuses when another tool has already disabled sleep. This helper exclusively owns that flag while installed; it restores it to `0`, not to another app's previous value. The CLI does not change normal AC/battery sleep timers, display sleep settings or security settings, and installs no passwordless sudo rule.

After enabling it, test closed-lid SSH **and physically unplugging your own Mac** before relying on it. Behavior can vary by Mac and macOS version. Never leave a running closed-lid laptop in a bag or another poorly ventilated space. Keep it on a hard, ventilated surface.

Recovery, if the service has failed and you need sleep back:

```sh
modafinil set --closed-lid off
# If that command fails, stop the helper first, then clear the flag manually:
sudo launchctl bootout system/dev.kitlangton.modafinil.lid
sudo pmset -a disablesleep 0
pmset -g
```

Do not just clear the flag while a healthy helper is running on AC; it will re-enable it.

## Status and removal

```sh
modafinil status --json
modafinil uninstall          # Removes launchd services and their installed copies.
cargo uninstall modafinil-cli
```

Uninstall retains settings and logs and clears the saved closed-lid preference. It asks for administrator approval only when the privileged helper is installed. `stop` refuses while the lid helper is installed; use `set --mode off` to stop both policies safely.

Files:

| File | Purpose |
|---|---|
| `~/.config/modafinil/settings.json` | Mode and closed-lid preference |
| `~/.local/share/modafinil/modafinil` | Login service binary |
| `~/Library/LaunchAgents/dev.kitlangton.modafinil.plist` | Login service |
| `~/Library/Logs/modafinil.log` | Login service log |
| `/Library/Application Support/Modafinil/modafinil` | Root-owned optional helper binary |
| `/Library/LaunchDaemons/dev.kitlangton.modafinil.lid.plist` | Optional boot service with fixed mode arguments |
| `/var/log/modafinil-lid.log` | Optional helper log |

Saved preferences and installed service state are reported separately. `awake_requested` is a policy result, not proof an assertion is active. `sleep_disabled` is read back from macOS and may have been set by another tool.

To update, run `cargo install modafinil-cli --locked --force`, then `modafinil install`. If using closed-lid support, also run `modafinil set --closed-lid on` to update the root-owned helper, with administrator approval.

## Development

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
cargo run -- check --power battery --mode always --closed-lid
cargo install --path . --locked
```

Tests cover the full policy matrix, strict power/settings parsing, safe plist rendering and CLI behavior without changing sleep settings. `check` is a dry-run policy evaluator and also runs on Linux. All other commands require macOS. Live privileged-helper and physical power/lid tests need a consenting operator; automated CI does not install services or change system sleep settings.

Ordinary assertions were also read back on a real Mac: duplicate runners were refused, malformed settings released the assertion, valid settings resumed it, and SIGTERM removed it. Privileged-helper recovery was tested through a mock system adapter. Root-owned installation, AC activation and graceful stop/restoration were checked on an Apple Silicon Mac. Physical unplugging and closed-lid operation of this Rust helper have not yet been tested on hardware. Treat that mode as experimental until those checks pass on your machine.

Version 0.1.1 waits for launchd to finish unloading services before reinstalling or removing files. A failed service-state read is an error, not evidence a service is absent.

## License

MIT. Copyright Kit Langton.
