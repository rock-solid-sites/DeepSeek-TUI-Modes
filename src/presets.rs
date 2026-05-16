/// Axis values that control which prompt fragments are assembled.
///
/// Each axis is optional — when `None`, no fragment is included for that
/// axis. This is the `none` mode (current behavior).
#[derive(Debug, Clone, Default)]
pub struct AxisValues {
    pub agency: Option<String>,
    pub quality: Option<String>,
    pub scope: Option<String>,
}

impl AxisValues {
    /// True when no axes are active (equivalent to `none` mode).
    pub fn is_empty(&self) -> bool {
        self.agency.is_none() && self.quality.is_none() && self.scope.is_none()
    }

    /// Merge another AxisValues into this one. Non-None values in `other`
    /// overwrite the corresponding values in `self`.
    pub fn merge(&mut self, other: &AxisValues) {
        if other.agency.is_some() {
            self.agency.clone_from(&other.agency);
        }
        if other.quality.is_some() {
            self.quality.clone_from(&other.quality);
        }
        if other.scope.is_some() {
            self.scope.clone_from(&other.scope);
        }
    }
}

/// Returns the axis values for a named preset, or `None` if the preset
/// is unknown.
pub fn get_preset(name: &str) -> Option<AxisValues> {
    match name {
        "none" => Some(AxisValues::default()),
        "safe" => Some(AxisValues {
            agency: Some("collaborative".to_string()),
            quality: Some("minimal".to_string()),
            scope: Some("narrow".to_string()),
        }),
        "create" | "muse" => Some(AxisValues {
            agency: Some("autonomous".to_string()),
            quality: Some("architect".to_string()),
            scope: Some("unrestricted".to_string()),
        }),
        "extend" => Some(AxisValues {
            agency: Some("autonomous".to_string()),
            quality: Some("pragmatic".to_string()),
            scope: Some("adjacent".to_string()),
        }),
        "refactor" => Some(AxisValues {
            agency: Some("autonomous".to_string()),
            quality: Some("pragmatic".to_string()),
            scope: Some("unrestricted".to_string()),
        }),
        "explore" => Some(AxisValues {
            agency: Some("collaborative".to_string()),
            quality: Some("architect".to_string()),
            scope: Some("narrow".to_string()),
        }),
        "debug" => Some(AxisValues {
            agency: Some("collaborative".to_string()),
            quality: Some("pragmatic".to_string()),
            scope: Some("narrow".to_string()),
        }),
        "methodical" => Some(AxisValues {
            agency: Some("surgical".to_string()),
            quality: Some("architect".to_string()),
            scope: Some("narrow".to_string()),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_none_preset() {
        let axes = get_preset("none").unwrap();
        assert!(axes.is_empty());
    }

    #[test]
    fn test_safe_preset() {
        let axes = get_preset("safe").unwrap();
        assert_eq!(axes.agency.as_deref(), Some("collaborative"));
        assert_eq!(axes.quality.as_deref(), Some("minimal"));
        assert_eq!(axes.scope.as_deref(), Some("narrow"));
    }

    #[test]
    fn test_create_preset() {
        let axes = get_preset("create").unwrap();
        assert_eq!(axes.agency.as_deref(), Some("autonomous"));
        assert_eq!(axes.quality.as_deref(), Some("architect"));
        assert_eq!(axes.scope.as_deref(), Some("unrestricted"));
    }

    #[test]
    fn test_muse_preset() {
        let axes = get_preset("muse").unwrap();
        assert_eq!(axes.agency.as_deref(), Some("autonomous"));
        assert_eq!(axes.quality.as_deref(), Some("architect"));
        assert_eq!(axes.scope.as_deref(), Some("unrestricted"));
    }

    #[test]
    fn test_extend_preset() {
        let axes = get_preset("extend").unwrap();
        assert_eq!(axes.agency.as_deref(), Some("autonomous"));
        assert_eq!(axes.quality.as_deref(), Some("pragmatic"));
        assert_eq!(axes.scope.as_deref(), Some("adjacent"));
    }

    #[test]
    fn test_refactor_preset() {
        let axes = get_preset("refactor").unwrap();
        assert_eq!(axes.agency.as_deref(), Some("autonomous"));
        assert_eq!(axes.quality.as_deref(), Some("pragmatic"));
        assert_eq!(axes.scope.as_deref(), Some("unrestricted"));
    }

    #[test]
    fn test_explore_preset() {
        let axes = get_preset("explore").unwrap();
        assert_eq!(axes.agency.as_deref(), Some("collaborative"));
        assert_eq!(axes.quality.as_deref(), Some("architect"));
        assert_eq!(axes.scope.as_deref(), Some("narrow"));
    }

    #[test]
    fn test_debug_preset() {
        let axes = get_preset("debug").unwrap();
        assert_eq!(axes.agency.as_deref(), Some("collaborative"));
        assert_eq!(axes.quality.as_deref(), Some("pragmatic"));
        assert_eq!(axes.scope.as_deref(), Some("narrow"));
    }

    #[test]
    fn test_methodical_preset() {
        let axes = get_preset("methodical").unwrap();
        assert_eq!(axes.agency.as_deref(), Some("surgical"));
        assert_eq!(axes.quality.as_deref(), Some("architect"));
        assert_eq!(axes.scope.as_deref(), Some("narrow"));
    }

    #[test]
    fn test_unknown_preset() {
        assert!(get_preset("bogus").is_none());
    }

    #[test]
    fn test_merge() {
        let mut base = AxisValues::default();
        let other = AxisValues {
            agency: Some("autonomous".to_string()),
            quality: None,
            scope: Some("unrestricted".to_string()),
        };
        base.merge(&other);
        assert_eq!(base.agency.as_deref(), Some("autonomous"));
        assert_eq!(base.quality, None);
        assert_eq!(base.scope.as_deref(), Some("unrestricted"));
    }

    #[test]
    fn test_merge_overwrite() {
        let mut base = AxisValues {
            agency: Some("collaborative".to_string()),
            quality: Some("minimal".to_string()),
            scope: Some("narrow".to_string()),
        };
        let other = AxisValues {
            agency: Some("autonomous".to_string()),
            quality: None,
            scope: None,
        };
        base.merge(&other);
        assert_eq!(base.agency.as_deref(), Some("autonomous"));
        assert_eq!(base.quality.as_deref(), Some("minimal"));
        assert_eq!(base.scope.as_deref(), Some("narrow"));
    }
}
