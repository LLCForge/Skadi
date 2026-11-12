# macOS privacy settings Skadi cannot change for you

[Türkçe](macos-privacy.tr.md)

Apple keeps a few privacy switches in System Settings only. Skadi covers the rest with tweaks and
`skadi macos-profile`; do these by hand.

## Install the Skadi profile

```bash
skadi macos-profile ~/Desktop/skadi.mobileconfig
```

Open the file, then approve it in **System Settings > General > Device Management** (or
**Profiles** on older versions). It turns off Spotlight internet suggestions, asks for your password
right after sleep or the screen saver, turns on the firewall with stealth mode, and reduces Chrome
and Firefox data collection. Remove the profile there to undo everything it set.

## Location Services

**System Settings > Privacy & Security > Location Services.** Turn it off completely, or leave it on
and switch off each app that does not need your location. Under **System Services > Details** you
can also turn off location-based suggestions and significant locations.

## Recent items

- Clear the lists now: **Apple menu > Recent Items > Clear Menu**.
- Keep fewer items: **System Settings > Control Center**, then set *Recent documents, applications,
  and servers* to a lower number.

## Check what apps can access

```bash
skadi permissions
```

This reads which apps have camera, microphone, screen recording and other permissions. macOS only
allows it after you give your terminal **Full Disk Access** in System Settings > Privacy & Security.
