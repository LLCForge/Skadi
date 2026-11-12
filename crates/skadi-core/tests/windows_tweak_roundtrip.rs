//! Gerçek registry üzerinde uygula → geri al döngüsü. Yalnızca Skadi'ye ait
//! `HKCU\Software\SkadiSelfTest` anahtarına dokunur ve sonunda onu siler.
//! Çalıştırmak için: `cargo test -p skadi-core --test windows_tweak_roundtrip -- --ignored`
#![cfg(windows)]

use std::process::Command;

use skadi_core::{
    Error,
    system::System,
    tweak::{StateStore, TweakCatalog, TweakEngine, TweakStatus},
};

const KEY: &str = "HKCU\\Software\\SkadiSelfTest";

fn reg(args: &[&str]) -> std::process::Output {
    Command::new("reg").args(args).output().expect("reg.exe çalışmalı")
}

fn query(name: &str) -> Option<String> {
    let out = reg(&["query", KEY, "/v", name]);
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[test]
#[ignore = "gerçek registry'ye yazar"]
fn apply_and_revert_restore_exact_previous_state() {
    let _ = reg(&["delete", KEY, "/f"]);
    // Önceden var olan değer: geri almada eski haline dönmeli.
    assert!(
        reg(&["add", KEY, "/v", "Existing", "/t", "REG_DWORD", "/d", "7", "/f"])
            .status
            .success()
    );
    // Tür uyuşmazlığı: DWORD beklenirken metin. Motor buna dokunmamalı.
    assert!(
        reg(&["add", KEY, "/v", "WrongType", "/t", "REG_SZ", "/d", "keep me", "/f"])
            .status
            .success()
    );

    let catalog = TweakCatalog::from_toml(&format!(
        r#"
        [[tweak]]
        id = "selftest"
        category = "system"
        risk = "safe"
        name = {{ en = "x", tr = "x" }}
        description = {{ en = "x", tr = "x" }}

        [[tweak.action]]
        type = "registry"
        key = '{KEY}'
        name = "Existing"
        kind = "dword"
        data = "0"

        [[tweak.action]]
        type = "registry"
        key = '{KEY}'
        name = "Created"
        kind = "string"
        data = "skadi"

        [[tweak]]
        id = "wrongtype"
        category = "system"
        risk = "safe"
        name = {{ en = "x", tr = "x" }}
        description = {{ en = "x", tr = "x" }}

        [[tweak.action]]
        type = "registry"
        key = '{KEY}'
        name = "WrongType"
        kind = "dword"
        data = "0"
        "#
    ))
    .unwrap();
    let tweak = catalog.get("selftest").unwrap();

    let system = System::detect().unwrap();
    let state_path = std::env::temp_dir()
        .join(format!("skadi-roundtrip-{}", std::process::id()))
        .join("state.toml");
    let mut store = StateStore::open_at(state_path.clone()).unwrap();
    let mut engine = TweakEngine::new(&system);

    assert_eq!(engine.status(tweak).unwrap(), TweakStatus::NotApplied);
    assert_eq!(engine.apply(tweak, &mut store).unwrap().changed, 2);
    assert_eq!(engine.status(tweak).unwrap(), TweakStatus::Applied);
    assert!(query("Existing").unwrap().contains("0x0"));
    assert!(query("Created").unwrap().contains("skadi"));

    // Kayıt diske yazıldı ve yeniden açıldığında geri alma çalışıyor.
    let mut store = StateStore::open_at(state_path.clone()).unwrap();
    let outcome = engine.revert(tweak, &mut store).unwrap();
    assert_eq!(outcome.restored, 2);
    assert!(query("Existing").unwrap().contains("0x7"), "eski değer geri yüklenmeli");
    assert!(query("Created").is_none(), "önceden olmayan değer silinmeli");
    assert!(!store.is_recorded("selftest"));
    assert!(matches!(
        engine.revert(tweak, &mut store),
        Err(Error::NothingToRevert(_))
    ));

    let wrong = catalog.get("wrongtype").unwrap();
    assert!(matches!(engine.apply(wrong, &mut store), Err(Error::Registry(_))));
    assert!(
        query("WrongType").unwrap().contains("keep me"),
        "farklı türdeki değere dokunulmamalı"
    );

    let _ = reg(&["delete", KEY, "/f"]);
    let _ = std::fs::remove_dir_all(state_path.parent().unwrap());
}

#[test]
#[ignore = "gerçek registry'ye yazar"]
fn revert_removes_only_keys_skadi_created_and_left_empty() {
    const ROOT: &str = r"HKCU\Software\SkadiSelfTest2";
    let _ = reg(&["delete", ROOT, "/f"]);
    let catalog = TweakCatalog::from_toml(&format!(
        r#"
        [[tweak]]
        id = "deep"
        category = "system"
        risk = "safe"
        name = {{ en = "x", tr = "x" }}
        description = {{ en = "x", tr = "x" }}

        [[tweak.action]]
        type = "registry"
        key = '{ROOT}\Deep\Leaf'
        name = ""
        kind = "string"
        data = ""
        "#
    ))
    .unwrap();
    let tweak = catalog.get("deep").unwrap();
    let system = System::detect().unwrap();
    let state = std::env::temp_dir()
        .join(format!("skadi-deep-{}", std::process::id()))
        .join("state.toml");
    let mut store = StateStore::open_at(state.clone()).unwrap();
    let mut engine = TweakEngine::new(&system);

    // 1) Anahtar hiç yokken: geri alma oluşturulan zinciri tamamen kaldırır.
    engine.apply(tweak, &mut store).unwrap();
    assert!(reg(&["query", &format!(r"{ROOT}\Deep\Leaf"), "/ve"]).status.success());
    engine.revert(tweak, &mut store).unwrap();
    assert!(
        !reg(&["query", ROOT]).status.success(),
        "Skadi'nin oluşturduğu boş anahtar kalmamalı"
    );

    // 2) Kullanıcı arada aynı ağaca kendi değerini eklerse anahtar silinmez.
    engine.apply(tweak, &mut store).unwrap();
    assert!(
        reg(&[
            "add",
            &format!(r"{ROOT}\Deep"),
            "/v",
            "Mine",
            "/t",
            "REG_SZ",
            "/d",
            "keep",
            "/f"
        ])
        .status
        .success()
    );
    engine.revert(tweak, &mut store).unwrap();
    let mine = reg(&["query", &format!(r"{ROOT}\Deep"), "/v", "Mine"]);
    assert!(mine.status.success(), "kullanıcının değeri korunmalı");

    let _ = reg(&["delete", ROOT, "/f"]);
    let _ = std::fs::remove_dir_all(state.parent().unwrap());
}
