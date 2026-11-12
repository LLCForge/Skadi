# Wi-Fi profillerini yeni bir bilgisayara taşımak

[English](wifi-profiles.md)

Skadi Wi-Fi profillerini kendisi dışa aktarmaz: başka bir bilgisayarda çalışacak bir profil, ağ
parolasını düz metin olarak içermek zorundadır. Gerektiğinde bunu elle yapın, dosyaları kimseyle
paylaşmayın ve içe aktardıktan sonra silin.

## Windows

Parolasız dışa aktarma (parolalar şifreli kalır ve yalnızca bu bilgisayarda çalışır):

```powershell
netsh wlan export profile folder="$env:USERPROFILE\wifi"
```

Başka bir bilgisayara taşımak için `key=clear` ekleyin. Bu durumda XML dosyaları parolaları düz metin
olarak içerir:

```powershell
netsh wlan export profile key=clear folder="$env:USERPROFILE\wifi"
```

Yeni bilgisayarda:

```powershell
netsh wlan add profile filename="$env:USERPROFILE\wifi\Wi-Fi-AgAdi.xml"
```

Ardından klasörü iki bilgisayardan da silin.

## Linux (NetworkManager)

Profiller `/etc/NetworkManager/system-connections/` klasöründedir ve parola içerdikleri için yalnızca
root tarafından okunabilir. Gereken `.nmconnection` dosyalarını yeni bilgisayarda aynı klasöre
kopyalayın, ardından:

```bash
sudo chown root:root /etc/NetworkManager/system-connections/*.nmconnection
sudo chmod 600 /etc/NetworkManager/system-connections/*.nmconnection
sudo nmcli connection reload
```

## macOS

Aynı Apple hesabıyla oturum açtığınızda Wi-Fi parolaları iCloud Anahtar Zinciri ile taşınır. iCloud
kullanmıyorsanız yeni Mac'te ağlara yeniden bağlanın.
