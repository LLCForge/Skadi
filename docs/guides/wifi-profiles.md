# Moving Wi-Fi profiles to a new machine

[Türkçe](wifi-profiles.tr.md)

Skadi does not export Wi-Fi profiles itself: a profile that works on another machine has to contain
the network password in plain text. Do it by hand when you need it, keep the files private and
delete them after importing.

## Windows

Export without passwords (they stay encrypted and only work on this PC):

```powershell
netsh wlan export profile folder="$env:USERPROFILE\wifi"
```

To move them to another PC, add `key=clear`. The XML files then contain the passwords in plain
text:

```powershell
netsh wlan export profile key=clear folder="$env:USERPROFILE\wifi"
```

On the new PC:

```powershell
netsh wlan add profile filename="$env:USERPROFILE\wifi\Wi-Fi-MyNetwork.xml"
```

Then delete the folder on both machines.

## Linux (NetworkManager)

Profiles live in `/etc/NetworkManager/system-connections/` and are readable only by root, because
they contain the password. Copy the `.nmconnection` files you need to the same folder on the new
machine, then:

```bash
sudo chown root:root /etc/NetworkManager/system-connections/*.nmconnection
sudo chmod 600 /etc/NetworkManager/system-connections/*.nmconnection
sudo nmcli connection reload
```

## macOS

Wi-Fi passwords move with iCloud Keychain when you sign in with the same Apple account. Without
iCloud, join the networks again on the new Mac.
