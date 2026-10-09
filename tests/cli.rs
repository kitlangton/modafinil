use std::process::Command;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_modafinil"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn help_and_version() {
    let help = cli(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for command in ["run", "status", "set", "install", "uninstall", "check"] {
        assert!(text.contains(command));
    }
    assert!(!text.contains("helper-install"));
    assert!(cli(&["--version"]).status.success());
}

#[test]
fn dry_policy_check_needs_no_privileges() {
    let output = cli(&[
        "check",
        "--power",
        "battery",
        "--mode",
        "always",
        "--closed-lid",
    ]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "awake=true closed_lid=false\n"
    );
    let output = cli(&["check", "--power", "unknown", "--closed-lid"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "awake=false closed_lid=false\n"
    );
}

#[test]
fn rejects_invalid_settings_and_zero_duration() {
    for args in [
        vec!["set", "--mode", "bogus"],
        vec!["set", "--closed-lid", "yes"],
        vec!["run", "--seconds", "0"],
    ] {
        assert!(!cli(&args).status.success());
    }
}
