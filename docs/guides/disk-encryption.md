# Disk encryption

[Türkçe](disk-encryption.tr.md)

Disk encryption protects your files if your computer is lost or stolen. `skadi audit` shows whether
it is on. Turning it on is done with the system's own tools; Skadi does not do it for you, because a
lost recovery key means lost data.

**First, back up your files.** Then keep the recovery key somewhere other than the computer.

## Windows: BitLocker and device encryption

- **Pro, Enterprise, Education:** Control Panel > System and Security > BitLocker Drive Encryption >
  Turn on BitLocker.
- **Home:** Settings > Privacy & security > Device encryption, on PCs that support it.

Check the status and see the recovery key in an administrator terminal:

```powershell
manage-bde -status C:
manage-bde -protectors -get C:
```

With a local account, Windows does not save the recovery key online; see the
[local account guide](local-account.md).

## Linux: LUKS

Encryption is easiest to set up when installing: choose the encrypted disk (LUKS) option in the
installer. Encrypting a disk that is already in use is possible with `cryptsetup reencrypt`, but
only with a full backup and by following your distribution's documentation.

Check the status:

```bash
lsblk -o NAME,TYPE,MOUNTPOINTS
```

A `crypt` entry above your root file system means it is encrypted. Keep a backup of the LUKS header
(`sudo cryptsetup luksHeaderBackup <device> --header-backup-file <file>`) somewhere safe.

## macOS: FileVault

System Settings > Privacy & Security > FileVault > Turn On. Keep the recovery key, or allow your
Apple account to unlock the disk.

```bash
fdesetup status
```
