# Skadi'nin sizin yerinize değiştiremediği macOS gizlilik ayarları

[English](macos-privacy.md)

Apple bazı gizlilik anahtarlarını yalnızca Sistem Ayarları'nda tutar. Skadi gerisini tweak'lerle ve
`skadi macos-profile` ile karşılar; bunları elle yapın.

## Skadi profilini kurun

```bash
skadi macos-profile ~/Desktop/skadi.mobileconfig
```

Dosyayı açın, ardından **Sistem Ayarları > Genel > Aygıt Yönetimi** (eski sürümlerde **Profiller**)
bölümünden onaylayın. Profil Spotlight internet önerilerini kapatır, uyku ya da ekran koruyucudan
hemen sonra parola ister, güvenlik duvarını gizli modla açar ve Chrome ile Firefox'un veri
toplamasını azaltır. Ayarladığı her şeyi geri almak için profili aynı yerden kaldırın.

## Konum Servisleri

**Sistem Ayarları > Gizlilik ve Güvenlik > Konum Servisleri.** Tamamen kapatın ya da açık bırakıp
konumunuza ihtiyacı olmayan her uygulamayı kapatın. **Sistem Servisleri > Ayrıntılar** bölümünden
konuma dayalı önerileri ve önemli konumları da kapatabilirsiniz.

## Son öğeler

- Listeleri hemen temizleyin: **Apple menüsü > Son Öğeler > Menüyü Temizle**.
- Daha az öğe tutun: **Sistem Ayarları > Denetim Merkezi** bölümünde *Son belgeler, uygulamalar ve
  sunucular* sayısını düşürün.

## Uygulamaların erişimini denetleyin

```bash
skadi permissions
```

Bu komut hangi uygulamaların kamera, mikrofon, ekran kaydı ve diğer izinlere sahip olduğunu okur.
macOS buna ancak Sistem Ayarları > Gizlilik ve Güvenlik bölümünden terminalinize **Tam Disk Erişimi**
verdiğinizde izin verir.
