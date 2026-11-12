# VPN and kill switch

[Türkçe](vpn.tr.md)

A VPN hides your traffic from your network and internet provider, and moves your trust to the VPN
provider. A **kill switch** blocks all traffic while the VPN is not connected, so nothing leaks when
the connection drops or the PC starts.

## Install a client with Skadi

```bash
skadi install mullvadvpn     # or: protonvpn, wireguard, openvpn
```

Prefer a provider that publishes its client's source code and has independent audits.

## Turn on the kill switch

| Client | Setting |
|---|---|
| Mullvad VPN | **Settings > VPN settings > Lockdown mode** blocks traffic whenever the VPN is off, including at startup. |
| Proton VPN | **Settings > Connection > Kill switch**. The advanced option also blocks traffic when you disconnect yourself. |
| WireGuard for Windows | Edit the tunnel and tick **Block untunneled traffic (kill-switch)**. It appears when the tunnel routes all traffic (`AllowedIPs = 0.0.0.0/0, ::/0`). |
| WireGuard on Linux (`wg-quick`) | Add firewall rules in `PostUp` and remove them in `PreDown`; the `wg-quick(8)` manual shows a kill switch example. |

Follow your provider's own documentation when it differs; client menus change between versions.

## Check for leaks

1. Connect the VPN and run `skadi dns status`. The DNS servers should be your VPN's, not your
   internet provider's.
2. Disconnect the VPN or unplug the network briefly: with the kill switch on, pages must stop
   loading.
3. Use your provider's leak test page for IPv6 and WebRTC leaks.

`skadi dns set` changes the DNS on your network adapters. VPN clients set their own DNS while
connected, so you can keep both.
