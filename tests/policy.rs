use modafinil_cli::{Mode, Power, Settings, parse_disabled, parse_power, plist, policy};

#[test]
fn default_is_ac_only_with_no_privileged_helper() {
    assert_eq!(
        Settings::default(),
        Settings {
            mode: Mode::Ac,
            closed_lid: false
        }
    );
}

#[test]
fn exhaustive_policy_matrix() {
    for mode in [Mode::Off, Mode::Ac, Mode::Always] {
        for power in [Power::Ac, Power::Battery, Power::Unknown] {
            for closed_lid in [false, true] {
                let result = policy(Settings { mode, closed_lid }, power);
                let expected = matches!(
                    (mode, power),
                    (Mode::Ac | Mode::Always, Power::Ac) | (Mode::Always, Power::Battery)
                );
                assert_eq!(result.awake, expected, "{mode:?} {power:?}");
                assert_eq!(
                    result.closed_lid,
                    expected && closed_lid && power == Power::Ac
                );
            }
        }
    }
}

#[test]
fn power_parsing_fails_closed() {
    assert_eq!(
        parse_power("Now drawing from 'AC Power'\n -InternalBattery-0\t100%; charged"),
        Power::Ac
    );
    assert_eq!(
        parse_power("Now drawing from 'Battery Power'\n -InternalBattery-0\t80%; discharging"),
        Power::Battery
    );
    for invalid in [
        "",
        "Now drawing from 'UPS Power'",
        "error AC Power",
        "\nNow drawing from 'AC Power'",
        "Now drawing from 'AC Power' bad",
    ] {
        assert_eq!(parse_power(invalid), Power::Unknown);
    }
}

#[test]
fn sleep_parsing_is_exact() {
    assert_eq!(
        parse_disabled("System-wide power settings:\n SleepDisabled\t\t1\n"),
        Some(true)
    );
    assert_eq!(parse_disabled(" SleepDisabled 0\n sleep 1"), Some(false));
    for invalid in [
        "",
        "SleepDisabled 2",
        "NotSleepDisabled 1",
        "SleepDisabled 1 unexpected",
        "sleep 0",
    ] {
        assert_eq!(parse_disabled(invalid), None);
    }
}

#[test]
fn settings_reject_unknown_or_invalid_values() {
    assert_eq!(
        serde_json::from_str::<Settings>("{}").unwrap(),
        Settings::default()
    );
    for invalid in [
        r#"{"mode":"sometimes"}"#,
        r#"{"closed_lid":"yes"}"#,
        r#"{"command":"/bin/sh"}"#,
        "not json",
    ] {
        assert!(serde_json::from_str::<Settings>(invalid).is_err());
    }
    let settings = Settings {
        mode: Mode::Always,
        closed_lid: true,
    };
    assert_eq!(
        serde_json::from_str::<Settings>(&serde_json::to_string(&settings).unwrap()).unwrap(),
        settings
    );
}

#[test]
fn launchd_paths_are_xml_escaped_without_shell_interpolation() {
    let xml = plist(
        "test.label",
        &["/Users/a & b/program".into(), "<arg>\"'".into()],
        "/tmp/a & b.log",
    );
    assert!(xml.contains("<string>/Users/a &amp; b/program</string>"));
    assert!(xml.contains("<string>&lt;arg&gt;&quot;&apos;</string>"));
    assert!(xml.contains("<integer>2</integer>"));
    assert!(!xml.contains("/bin/sh"));
}
