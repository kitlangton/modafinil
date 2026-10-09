use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Off,
    #[default]
    Ac,
    Always,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Ac => "ac",
            Self::Always => "always",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    Ac,
    Battery,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub closed_lid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    pub awake: bool,
    pub closed_lid: bool,
}

/// Unknown power always releases assertions, including in Always mode.
pub fn policy(settings: Settings, power: Power) -> Policy {
    let awake = match power {
        Power::Unknown => false,
        Power::Ac => settings.mode != Mode::Off,
        Power::Battery => settings.mode == Mode::Always,
    };
    Policy {
        awake,
        closed_lid: awake && settings.closed_lid && power == Power::Ac,
    }
}

pub fn parse_power(output: &str) -> Power {
    match output.lines().next() {
        Some("Now drawing from 'AC Power'") => Power::Ac,
        Some("Now drawing from 'Battery Power'") => Power::Battery,
        _ => Power::Unknown,
    }
}

pub fn parse_disabled(output: &str) -> Option<bool> {
    output.lines().find_map(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["SleepDisabled", "0"] => Some(false),
            ["SleepDisabled", "1"] => Some(true),
            _ => None,
        }
    })
}

/// System adapter kept separate so failure/recovery behavior is testable without privileges.
pub trait SleepControl {
    fn disabled(&mut self) -> Result<bool, String>;
    fn set_disabled(&mut self, value: bool) -> Result<(), String>;
}

pub fn reconcile_sleep(control: &mut impl SleepControl, desired: bool) -> Result<(), String> {
    let result = control.disabled().and_then(|actual| {
        if actual == desired {
            Ok(())
        } else {
            control.set_disabled(desired)
        }
    });
    if let Err(error) = result {
        return match control.set_disabled(false) {
            Ok(()) => Err(format!("{error}; sleep restored")),
            Err(recovery) => Err(format!(
                "{error}; restoration failed: {recovery}; retrying next check"
            )),
        };
    }
    Ok(())
}

pub fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn plist(label: &str, args: &[String], log: &str) -> String {
    let args = args
        .iter()
        .map(|arg| format!("<string>{}</string>", xml_escape(arg)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>Label</key><string>{}</string>\n\
         <key>ProgramArguments</key><array>{args}</array>\n\
         <key>RunAtLoad</key><true/>\n\
         <key>KeepAlive</key><true/>\n\
         <key>ThrottleInterval</key><integer>2</integer>\n\
         <key>StandardOutPath</key><string>{}</string>\n\
         <key>StandardErrorPath</key><string>{}</string>\n\
         </dict></plist>\n",
        xml_escape(label),
        xml_escape(log),
        xml_escape(log),
    )
}
