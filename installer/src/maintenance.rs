use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub present: bool,
    pub current_version: Option<String>,
    pub previous_version: Option<String>,
}
impl Installation {
    pub fn allows(&self, action: &str) -> bool {
        match action {
            "install" => !self.present,
            "update" => self.present && self.current_version.is_some(),
            "repair" | "uninstall" => self.present,
            "rollback" => self.present && self.previous_version.is_some(),
            _ => false,
        }
    }
    pub fn default_action(&self) -> &'static str {
        if !self.present {
            "install"
        } else if self.current_version.is_some() {
            "update"
        } else {
            "repair"
        }
    }
    pub fn same_package(&self, name: &str) -> bool {
        self.current_version.as_ref().is_some_and(|version| {
            name == format!("framely-{version}-linux-arm64.tar.gz")
                || name == format!("framely-{version}-offline-linux-arm64.tar.gz")
        })
    }
}
pub fn requires_package(action: &str) -> bool {
    matches!(action, "install" | "update")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operations_follow_device_state_and_previous_version() {
        let missing = Installation::default();
        assert_eq!(missing.default_action(), "install");
        for action in ["update", "repair", "uninstall", "rollback", "unknown"] {
            assert!(!missing.allows(action));
        }
        assert!(missing.allows("install"));
        let mut installed = Installation {
            present: true,
            current_version: Some("0.4.2-preview.3-build".into()),
            previous_version: None,
        };
        assert_eq!(installed.default_action(), "update");
        assert!(!installed.allows("install"));
        assert!(!installed.allows("rollback"));
        for action in ["update", "repair", "uninstall"] {
            assert!(installed.allows(action));
        }
        installed.previous_version = Some("0.4.1-build".into());
        assert!(installed.allows("rollback"));
        assert!(installed.same_package("framely-0.4.2-preview.3-build-linux-arm64.tar.gz"));
        assert!(!installed.same_package("framely-0.4.2-preview.3-other-linux-arm64.tar.gz"));
        installed.current_version = None;
        assert_eq!(installed.default_action(), "repair");
        assert!(!installed.allows("update"));
        assert!(!installed.allows("install"));
        for action in ["repair", "rollback", "uninstall"] {
            assert!(!requires_package(action));
        }
        for action in ["install", "update"] {
            assert!(requires_package(action));
        }
    }
}
