# Disk şifreleme

[English](disk-encryption.md)

Disk şifreleme, bilgisayarınız kaybolduğunda ya da çalındığında dosyalarınızı korur. `skadi audit`
şifrelemenin açık olup olmadığını gösterir. Şifreleme sistemin kendi araçlarıyla açılır; kurtarma
anahtarının kaybı veri kaybı demek olduğu için Skadi bunu sizin yerinize yapmaz.

**Önce dosyalarınızı yedekleyin.** Kurtarma anahtarını da bilgisayardan başka bir yerde saklayın.

## Windows: BitLocker ve cihaz şifreleme

- **Pro, Enterprise, Education:** Denetim Masası > Sistem ve Güvenlik > BitLocker Sürücü Şifrelemesi >
  BitLocker'ı aç.
- **Home:** Destekleyen bilgisayarlarda Ayarlar > Gizlilik ve güvenlik > Cihaz şifreleme.

Durumu ve kurtarma anahtarını yönetici terminalinde görebilirsiniz:

```powershell
manage-bde -status C:
manage-bde -protectors -get C:
```

Yerel hesap kullanıyorsanız Windows kurtarma anahtarını çevrimiçi kaydetmez; bkz.
[yerel hesap rehberi](local-account.tr.md).

## Linux: LUKS

Şifreleme en kolay kurulum sırasında yapılır: kurulum programında şifreli disk (LUKS) seçeneğini
seçin. Kullanımdaki bir diski `cryptsetup reencrypt` ile şifrelemek mümkündür, ancak yalnızca tam bir
yedekle ve dağıtımınızın belgelerini izleyerek yapın.

Durumu denetleme:

```bash
lsblk -o NAME,TYPE,MOUNTPOINTS
```

Kök dosya sisteminizin üstünde `crypt` satırı varsa disk şifrelidir. LUKS başlığının bir yedeğini
(`sudo cryptsetup luksHeaderBackup <aygıt> --header-backup-file <dosya>`) güvenli bir yerde saklayın.

## macOS: FileVault

Sistem Ayarları > Gizlilik ve Güvenlik > FileVault > Aç. Kurtarma anahtarını saklayın ya da diskin
kilidini Apple hesabınızın açmasına izin verin.

```bash
fdesetup status
```
