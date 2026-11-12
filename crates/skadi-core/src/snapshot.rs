//! Değişikliklerden önce sistem anlık görüntüsü: Windows geri yükleme noktası, Linux'ta
//! Timeshift ya da Snapper, macOS'ta Time Machine yerel anlık görüntüsü.

use std::ffi::OsString;

use crate::{
    error::{Error, Result},
    exec::{Invocation, Privilege, find_program},
    platform::Os,
    system::System,
};

/// Anlık görüntüyü alacak komut ve kullanılan aracın adı; uygun araç yoksa `None`.
pub fn snapshot_invocation(system: &System) -> Result<Option<(&'static str, Invocation)>> {
    let (tool, program, args): (&'static str, &'static str, Vec<&str>) = match system.os {
        Os::Windows => (
            "System Restore",
            "powershell",
            vec![
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Checkpoint-Computer -Description 'Skadi' -RestorePointType 'MODIFY_SETTINGS' -ErrorAction Stop",
            ],
        ),
        Os::Linux if find_program("timeshift").is_some() => (
            "Timeshift",
            "timeshift",
            vec!["--create", "--comments", "Skadi", "--scripted"],
        ),
        Os::Linux if find_program("snapper").is_some() => {
            ("Snapper", "snapper", vec!["create", "--description", "Skadi"])
        }
        Os::Linux => return Ok(None),
        Os::Macos => ("Time Machine", "tmutil", vec!["localsnapshot"]),
    };
    let path = find_program(program).ok_or(Error::ProgramNotFound(program))?;
    let invocation = Invocation::build(
        path,
        args.into_iter().map(OsString::from).collect(),
        &[],
        Privilege::Root,
        &system.privileges,
        system.os,
    )?;
    Ok(Some((tool, invocation)))
}
