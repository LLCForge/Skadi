//! `skadi sign` ve `skadi policy`: anahtar üretimi, imzalama, doğrulama ve kurum politikası.

use std::path::Path;

use skadi_core::{
    Error, Lang, Result,
    platform::Os,
    policy::{self, POLICY_FILE, PUBLIC_KEY_FILE},
    signing,
    system::System,
};

use crate::commands::confirm;

fn io_error(path: &Path) -> impl Fn(std::io::Error) -> Error + '_ {
    move |e| Error::State(format!("{}: {e}", path.display()))
}

/// Gizli anahtarı yalnızca sahibinin okuyabileceği izinlerle yazar.
fn write_private(path: &Path, text: &str) -> Result<()> {
    if path.exists() {
        return Err(Error::State(format!(
            "{}: already exists; refusing to overwrite a key",
            path.display()
        )));
    }
    std::fs::write(path, text).map_err(io_error(path))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(io_error(path))?;
    }
    Ok(())
}

pub fn keygen(lang: Lang, name: &Path) -> Result<()> {
    let (secret, public) = signing::generate()?;
    let mut key = name.as_os_str().to_owned();
    key.push(".key");
    let mut pubkey = name.as_os_str().to_owned();
    pubkey.push(".pub");
    let (key, pubkey) = (std::path::PathBuf::from(key), std::path::PathBuf::from(pubkey));
    write_private(&key, &format!("{secret}\n"))?;
    std::fs::write(&pubkey, format!("{public}\n")).map_err(io_error(&pubkey))?;
    println!(
        "{} {}\n{} {}\n{}",
        lang.pick("Secret key:", "Gizli anahtar:"),
        key.display(),
        lang.pick("Public key:", "Açık anahtar:"),
        pubkey.display(),
        lang.pick(
            "Keep the secret key private and offline; share only the .pub file.",
            "Gizli anahtarı kimseyle paylaşmayın ve çevrimdışı saklayın; yalnızca .pub dosyasını paylaşın."
        )
    );
    Ok(())
}

pub fn sign_file(lang: Lang, file: &Path, key: &Path) -> Result<()> {
    let data = std::fs::read(file).map_err(io_error(file))?;
    let secret = std::fs::read_to_string(key).map_err(io_error(key))?;
    let signature = signing::sign(&secret, &data)?;
    let target = signing::signature_path(file);
    std::fs::write(&target, format!("{signature}\n")).map_err(io_error(&target))?;
    println!(
        "{} {}",
        lang.pick("Signature written to", "İmza yazıldı:"),
        target.display()
    );
    Ok(())
}

pub fn verify_file(lang: Lang, file: &Path, public_key: &Path) -> Result<()> {
    signing::read_verified(file, public_key)?;
    println!("{}", lang.pick("Signature is valid.", "İmza geçerli."));
    Ok(())
}

/// Politikayı gösterir, kurar ya da kaldırır. Kurulum önce imzayı doğrular, sonra dosyaları
/// makine genelindeki klasöre kopyalar; bu yönetici yetkisi ister.
pub fn policy(lang: Lang, action: &str, file: Option<&Path>, public_key: Option<&Path>, yes: bool) -> Result<()> {
    let t = |en, tr| lang.pick(en, tr);
    let system = System::detect()?;
    let dir = policy::policy_dir(system.os);
    match action {
        "show" => {
            match policy::load(system.os)? {
                None => println!("{}", t("No policy is installed.", "Kurulu politika yok.")),
                Some(p) => {
                    let list = |items: &Option<Vec<String>>| {
                        items
                            .as_ref()
                            .map_or_else(|| t("any", "hepsi").to_owned(), |v| v.join(", "))
                    };
                    println!("{} {}", t("Policy:", "Politika:"), dir.display());
                    println!(
                        "  {:<16} {}",
                        t("Allowed apps", "İzinli uygulamalar"),
                        list(&p.allowed_apps)
                    );
                    println!(
                        "  {:<16} {}",
                        t("Allowed tweaks", "İzinli tweak'ler"),
                        list(&p.allowed_tweaks)
                    );
                }
            }
            Ok(())
        }
        "install" => {
            let (Some(file), Some(key)) = (file, public_key) else {
                return Err(Error::InvalidOption("policy install FILE --public-key FILE"));
            };
            require_admin(lang, &system)?;
            let data = signing::read_verified(file, key)?;
            let text = String::from_utf8(data.clone()).map_err(|_| Error::Policy("policy is not UTF-8".into()))?;
            let parsed = policy::parse(&text)?;
            println!("{parsed:?}");
            if !confirm(
                lang,
                yes,
                t(
                    "Install this policy for everyone on this computer?",
                    "Bu politika bu bilgisayardaki herkes için kurulsun mu?",
                ),
            )? {
                return Ok(());
            }
            std::fs::create_dir_all(&dir).map_err(io_error(&dir))?;
            let signature = std::fs::read(signing::signature_path(file)).map_err(io_error(file))?;
            let public = std::fs::read(key).map_err(io_error(key))?;
            std::fs::write(dir.join(POLICY_FILE), &data).map_err(io_error(&dir))?;
            std::fs::write(signing::signature_path(&dir.join(POLICY_FILE)), signature).map_err(io_error(&dir))?;
            std::fs::write(dir.join(PUBLIC_KEY_FILE), public).map_err(io_error(&dir))?;
            policy::load(system.os)?;
            println!("{} {}", t("Policy installed in", "Politika kuruldu:"), dir.display());
            Ok(())
        }
        _ => {
            require_admin(lang, &system)?;
            if !confirm(
                lang,
                yes,
                t("Remove the organisation policy?", "Kurum politikası kaldırılsın mı?"),
            )? {
                return Ok(());
            }
            if dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(io_error(&dir))?;
            }
            println!("{}", t("Policy removed.", "Politika kaldırıldı."));
            Ok(())
        }
    }
}

fn require_admin(lang: Lang, system: &System) -> Result<()> {
    if system.privileges.is_elevated {
        return Ok(());
    }
    Err(Error::State(
        if system.os == Os::Windows {
            lang.pick("run Skadi as administrator", "Skadi'yi yönetici olarak çalıştırın")
        } else {
            lang.pick("run with sudo", "sudo ile çalıştırın")
        }
        .to_owned(),
    ))
}
