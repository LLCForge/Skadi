# Windows'u yerel hesapla kullanmak

[English](local-account.md)

Yerel hesap, oturum açma bilgilerinizi bir Microsoft hesabına bağlamak yerine bilgisayarınızda
tutar. Skadi iki hesap türüyle de aynı şekilde çalışır.

## Mevcut Microsoft hesabını yerel hesaba çevirme

Microsoft'un desteklediği yöntem budur ve tüm Windows 11 sürümlerinde çalışır.

1. **Ayarlar > Hesaplar > Bilgileriniz** sayfasını açın.
2. **Bunun yerine yerel hesapla oturum açın** seçeneğine tıklayıp adımları izleyin.
3. Oturumu kapatıp yeni yerel kullanıcı adı ve parolayla yeniden açın.

Dosyalarınız ve uygulamalarınız yerinde kalır.

## Kurulum sırasında

Microsoft kurulum ekranlarını sık değiştiriyor ve Microsoft hesabını atlatan kısayolları kaldırıyor.
İnternette bulduğunuz yöntemler sizin Windows sürümünüzde çalışmayabilir. Güvenilir yollar:

- Kurulumu Microsoft hesabıyla bitirip yukarıdaki gibi yerel hesaba geçin.
- Birden çok bilgisayar için yerel hesap oluşturan bir yanıt dosyası (`autounattend.xml`) kullanın.
  Bu, belgelenmiş bir Windows dağıtım özelliğidir.

## Geçmeden önce: BitLocker kurtarma anahtarı

Microsoft hesabıyla Windows, BitLocker ya da cihaz şifreleme kurtarma anahtarını o hesaba kaydeder.
Yerel hesapta bunu yapmaz. Geçmeden önce anahtarı kendiniz kaydedin:

```powershell
manage-bde -protectors -get C:
```

Komutu yönetici terminalinde çalıştırın ve **Sayısal Parola** değerini bilgisayardan ayrı, güvenli
bir yerde saklayın (örneğin kâğıda yazdırarak ya da bir parola yöneticisinde). Bu anahtar olmadan bir
donanım değişikliği ya da firmware güncellemesi sizi kendi diskinizin dışında bırakabilir.

## Neler değişir

- OneDrive, Microsoft Store satın alımları ve ayar eşitleme için her uygulamada ayrıca oturum açmanız
  gerekir.
- Windows Hello (PIN, yüz, parmak izi) çalışmaya devam eder.
- `skadi audit`, parolasız oturum açabilen yerel hesapları denetler.
