//! Seçilen uygulamaları paket yöneticilerine dağıtan saf (yan etkisiz) planlayıcı.
//! Arayüzler planı kullanıcıya gösterip onay aldıktan sonra çalıştırır.

use crate::{catalog::App, manager::ManagerId, platform::Os};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepItem {
    pub app: String,
    pub package: String,
}

/// Tek bir paket yöneticisiyle yapılacak iş.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub manager: ManagerId,
    pub items: Vec<StepItem>,
}

/// Bu sistemde mevcut hiçbir yöneticiyle kurulamayan uygulama.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    pub app: String,
    /// Kurulursa bu uygulamayı sağlayabilecek yöneticiler (tercih sırasıyla).
    pub candidates: Vec<ManagerId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    pub unresolved: Vec<Unresolved>,
}

/// `available` tercih sırasına göre dizilmiş, sistemde bulunan yöneticilerdir.
/// `forced` verilirse yalnızca o yönetici kullanılır.
pub fn plan(apps: &[&App], os: Os, available: &[ManagerId], forced: Option<ManagerId>) -> Plan {
    let allowed = |id: &ManagerId| forced.is_none_or(|f| f == *id);
    let mut result = Plan::default();
    for app in apps {
        let choice = available
            .iter()
            .filter(|id| allowed(id))
            .find_map(|id| app.packages.get(id).map(|package| (*id, package)));
        match choice {
            Some((manager, package)) => {
                let item = StepItem {
                    app: app.id.clone(),
                    package: package.clone(),
                };
                match result.steps.iter_mut().find(|step| step.manager == manager) {
                    Some(step) => step.items.push(item),
                    None => result.steps.push(Step {
                        manager,
                        items: vec![item],
                    }),
                }
            }
            None => result.unresolved.push(Unresolved {
                app: app.id.clone(),
                candidates: ManagerId::ALL
                    .into_iter()
                    .filter(|id| id.spec().os == os && allowed(id) && app.packages.contains_key(id))
                    .collect(),
            }),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    fn catalog() -> Catalog {
        Catalog::from_toml(
            r#"
            [[app]]
            id = "git"
            name = "Git"
            category = "development"
            description = { en = "x", tr = "x" }
            [app.packages]
            apt = "git"
            flatpak = "org.example.Git"

            [[app]]
            id = "editor"
            name = "Editor"
            category = "development"
            description = { en = "x", tr = "x" }
            [app.packages]
            flatpak = "org.example.Editor"
            snap = "editor"

            [[app]]
            id = "winonly"
            name = "WinOnly"
            category = "utilities"
            description = { en = "x", tr = "x" }
            [app.packages]
            winget = "Win.Only"
            "#,
        )
        .unwrap()
    }

    #[test]
    fn groups_by_preferred_available_manager() {
        let catalog = catalog();
        let apps: Vec<&App> = catalog.apps().iter().collect();
        let plan = plan(&apps, Os::Linux, &[ManagerId::Apt, ManagerId::Flatpak], None);

        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].manager, ManagerId::Apt);
        assert_eq!(plan.steps[0].items[0].package, "git");
        assert_eq!(plan.steps[1].manager, ManagerId::Flatpak);
        assert_eq!(plan.steps[1].items[0].app, "editor");
        // Windows'a özel uygulama Linux'ta aday üretmez.
        assert_eq!(
            plan.unresolved,
            [Unresolved {
                app: "winonly".into(),
                candidates: vec![]
            }]
        );
    }

    #[test]
    fn reports_bootstrappable_candidates() {
        let catalog = catalog();
        let apps = [catalog.get("editor").unwrap()];
        let plan = plan(&apps, Os::Linux, &[ManagerId::Apt], None);
        assert!(plan.steps.is_empty());
        assert_eq!(plan.unresolved[0].candidates, [ManagerId::Flatpak, ManagerId::Snap]);
    }

    #[test]
    fn forced_manager_restricts_choice() {
        let catalog = catalog();
        let apps = [catalog.get("git").unwrap()];
        let plan = plan(
            &apps,
            Os::Linux,
            &[ManagerId::Apt, ManagerId::Flatpak],
            Some(ManagerId::Flatpak),
        );
        assert_eq!(plan.steps[0].manager, ManagerId::Flatpak);
    }
}
