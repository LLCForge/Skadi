# Otomatik güvenlik güncellemeleri

[English](automatic-updates.md)

Güvenlik düzeltmeleri ancak kurulduklarında işe yarar. `skadi audit`, otomatik güvenlik
güncellemelerinin açık olup olmadığını gösterir. Her dağıtım bunu farklı yapılandırdığı için Skadi
bu ayarı kendiliğinden açmaz; resmi yollar şunlardır.

## Debian ve Ubuntu

```bash
sudo apt install unattended-upgrades
sudo dpkg-reconfigure --priority=low unattended-upgrades
```

Kararlı güncellemelerin otomatik indirilip kurulması sorusuna **Evet** deyin. Ubuntu'da bu çoğunlukla
zaten açıktır.

## Fedora

Fedora 41 ve sonrası (dnf5):

```bash
sudo dnf install dnf5-plugin-automatic
```

`/etc/dnf/automatic.conf` dosyasında `apply_updates = yes` yapın, ardından:

```bash
sudo systemctl enable --now dnf5-automatic.timer
```

Eski sürümler (dnf4) `dnf-automatic` paketini ve
`sudo systemctl enable --now dnf-automatic-install.timer` komutunu kullanır.

## Arch Linux ve diğer sürekli güncellenen dağıtımlar

Arch, bazı güncellemeler elle adım gerektirdiği için güncellemeyi elle yapmayı ve önce duyuruları
okumayı önerir. `skadi update` komutunu düzenli çalıştırın ya da haftalık güncelleme için
`skadi schedule enable` kullanın.

## Windows ve macOS

- **Windows:** Windows Update güvenlik güncellemelerini otomatik kurar. Kapatmayın; Skadi'nin
  güncelleme ayarları güvenlik güncellemelerini asla durdurmaz.
- **macOS:** Sistem Ayarları > Genel > Yazılım Güncelleme > Otomatik güncellemeler bölümünde
  **Güvenlik Yanıtlarını ve sistem dosyalarını yükle** seçeneğini açın.

Skadi ile kurulan uygulamalar için `skadi schedule enable` haftalık bir `skadi update` kurar.
