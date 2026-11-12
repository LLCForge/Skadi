# VPN ve kill switch

[English](vpn.md)

VPN, trafiğinizi ağınızdan ve internet sağlayıcınızdan gizler; güveninizi VPN sağlayıcısına taşır.
**Kill switch** ise VPN bağlı değilken tüm trafiği engeller. Böylece bağlantı koptuğunda ya da
bilgisayar açılırken hiçbir veri sızmaz.

## Skadi ile istemci kurma

```bash
skadi install mullvadvpn     # ya da: protonvpn, wireguard, openvpn
```

İstemcisinin kaynak kodunu yayımlayan ve bağımsız denetimden geçmiş bir sağlayıcıyı tercih edin.

## Kill switch'i açma

| İstemci | Ayar |
|---|---|
| Mullvad VPN | **Ayarlar > VPN ayarları > Kilitleme modu (Lockdown mode)**: VPN kapalıyken, açılış dahil, trafiği engeller. |
| Proton VPN | **Ayarlar > Bağlantı > Kill switch**. Gelişmiş seçenek, bağlantıyı kendiniz kestiğinizde de trafiği engeller. |
| Windows için WireGuard | Tüneli düzenleyip **Block untunneled traffic (kill-switch)** kutusunu işaretleyin. Tünel tüm trafiği yönlendirdiğinde (`AllowedIPs = 0.0.0.0/0, ::/0`) görünür. |
| Linux'ta WireGuard (`wg-quick`) | Güvenlik duvarı kurallarını `PostUp` ile ekleyip `PreDown` ile kaldırın; `wg-quick(8)` kılavuzunda bir kill switch örneği vardır. |

Sağlayıcınızın belgeleri farklıysa onları izleyin; istemci menüleri sürümden sürüme değişir.

## Sızıntı denetimi

1. VPN'e bağlanıp `skadi dns status` çalıştırın. DNS sunucuları internet sağlayıcınızın değil, VPN'in
   sunucuları olmalı.
2. VPN bağlantısını kesin ya da ağ kablosunu kısa süre çıkarın: kill switch açıksa sayfalar
   yüklenmemeli.
3. IPv6 ve WebRTC sızıntıları için sağlayıcınızın sızıntı testi sayfasını kullanın.

`skadi dns set` ağ bağdaştırıcılarınızın DNS'ini değiştirir. VPN istemcileri bağlıyken kendi DNS'lerini
kullanır, bu yüzden ikisini birlikte kullanabilirsiniz.
