//! Denetim günlüğü: çalıştırılan komutlar JSON satırları olarak kaydedilir.

use std::ffi::OsString;

use skadi_core::exec::{Invocation, find_program, set_audit_log};

#[test]
fn executed_commands_are_appended_to_the_audit_log() {
    let dir = std::env::temp_dir().join(format!("skadi-audit-{}", std::process::id()));
    let log = dir.join("audit.log");
    set_audit_log(log.clone());
    let rustc = find_program("rustc").expect("rustc bulunmalı");
    let invocation = Invocation {
        program: rustc,
        args: vec![OsString::from("--version")],
        env: Vec::new(),
    };
    invocation.run_captured().unwrap();
    invocation.run_captured().unwrap();

    let text = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["ok"], true);
    assert_eq!(lines[0]["args"][0], "--version");
    assert!(lines[0]["time"].as_u64().unwrap() > 0);
    let _ = std::fs::remove_dir_all(dir);
}
