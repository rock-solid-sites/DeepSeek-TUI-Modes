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
        _ => None,
    }
}
