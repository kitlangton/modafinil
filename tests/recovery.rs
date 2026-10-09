use modafinil_cli::{SleepControl, reconcile_sleep};

#[derive(Default)]
struct Fake {
    actual: bool,
    read_fails: bool,
    write_failures: usize,
    writes: Vec<bool>,
}
impl SleepControl for Fake {
    fn disabled(&mut self) -> Result<bool, String> {
        if self.read_fails {
            Err("read timeout".into())
        } else {
            Ok(self.actual)
        }
    }
    fn set_disabled(&mut self, value: bool) -> Result<(), String> {
        self.writes.push(value);
        if self.write_failures > 0 {
            self.write_failures -= 1;
            return Err("write timeout".into());
        }
        self.actual = value;
        Ok(())
    }
}

#[test]
fn unchanged_flag_is_not_rewritten() {
    let mut fake = Fake::default();
    reconcile_sleep(&mut fake, false).unwrap();
    assert!(fake.writes.is_empty());
}

#[test]
fn failed_read_restores_without_enabling() {
    let mut fake = Fake {
        actual: true,
        read_fails: true,
        ..Default::default()
    };
    assert!(reconcile_sleep(&mut fake, true).is_err());
    assert_eq!(fake.writes, vec![false]);
    assert!(!fake.actual);
}

#[test]
fn failed_enable_attempts_restoration() {
    let mut fake = Fake {
        write_failures: 1,
        ..Default::default()
    };
    assert!(reconcile_sleep(&mut fake, true).is_err());
    assert_eq!(fake.writes, vec![true, false]);
    assert!(!fake.actual);
}

#[test]
fn failed_recovery_is_retried_instead_of_aborting() {
    let mut fake = Fake {
        actual: true,
        write_failures: 2,
        ..Default::default()
    };
    assert!(
        reconcile_sleep(&mut fake, false)
            .unwrap_err()
            .contains("restoration failed")
    );
    assert!(fake.actual);
    reconcile_sleep(&mut fake, false).unwrap();
    assert!(!fake.actual);
    assert_eq!(fake.writes, vec![false, false, false]);
}

#[test]
fn normal_ac_to_battery_transition_clears_the_flag() {
    let mut fake = Fake::default();
    reconcile_sleep(&mut fake, true).unwrap();
    reconcile_sleep(&mut fake, false).unwrap();
    assert_eq!(fake.writes, vec![true, false]);
    assert!(!fake.actual);
}
