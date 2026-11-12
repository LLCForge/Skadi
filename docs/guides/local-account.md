# Using Windows with a local account

[Türkçe](local-account.tr.md)

A local account keeps your sign-in on your PC instead of tying it to a Microsoft account. Skadi
works the same with both.

## Switch an existing Microsoft account to a local account

This is the method Microsoft supports and it works on every Windows 11 edition.

1. Open **Settings > Accounts > Your info**.
2. Select **Sign in with a local account instead** and follow the steps.
3. Sign out and back in with the new local user name and password.

Your files and apps stay where they are.

## During setup

Microsoft changes the setup screens often and has been removing the shortcuts that skipped the
Microsoft account. Tricks you find online may not work on your Windows version. The reliable ways
are:

- Finish setup with a Microsoft account, then switch as above.
- For several PCs, use an answer file (`autounattend.xml`) that creates a local account. This is a
  documented Windows deployment feature.

## Before you switch: the BitLocker recovery key

With a Microsoft account, Windows saves the BitLocker or device encryption recovery key to that
account. With a local account it does not. Save the key yourself before switching:

```powershell
manage-bde -protectors -get C:
```

Run it in an administrator terminal and keep the **Numerical Password** somewhere safe, away from
the PC (for example printed, or in a password manager). Without it, a hardware change or a firmware
update can lock you out of your own disk.

## What changes

- OneDrive, Microsoft Store purchases and settings sync need you to sign in to each app separately.
- Windows Hello (PIN, face, fingerprint) still works.
- `skadi audit` checks for local accounts that can sign in without a password.
