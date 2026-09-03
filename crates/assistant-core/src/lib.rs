use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub household_name: String,
    pub theme: Theme,
    pub launch_at_login: bool,
    pub store_complete_email_content: bool,
    pub local_email_analysis_enabled: bool,
    pub automation_policy: AutomationPolicy,
    pub notification_delivery_enabled: bool,
    pub urgent_alerts_enabled: bool,
    pub appointment_reminders_enabled: bool,
    pub morning_summary_enabled: bool,
    pub evening_summary_enabled: bool,
    pub quiet_hours_start_minute: u16,
    pub quiet_hours_end_minute: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            household_name: "My household".into(),
            theme: Theme::System,
            launch_at_login: false,
            store_complete_email_content: false,
            local_email_analysis_enabled: false,
            automation_policy: AutomationPolicy::Balanced,
            notification_delivery_enabled: false,
            urgent_alerts_enabled: false,
            appointment_reminders_enabled: false,
            morning_summary_enabled: false,
            evening_summary_enabled: false,
            quiet_hours_start_minute: 22 * 60,
            quiet_hours_end_minute: 7 * 60,
        }
    }
}

impl Settings {
    pub fn validate(self) -> Result<Self, ValidationError> {
        let household_name = self.household_name.trim().to_owned();
        if household_name.is_empty() || household_name.chars().count() > 80 {
            return Err(ValidationError::InvalidHouseholdName);
        }
        if self.quiet_hours_start_minute >= 1440
            || self.quiet_hours_end_minute >= 1440
            || self.quiet_hours_start_minute == self.quiet_hours_end_minute
        {
            return Err(ValidationError::InvalidQuietHours);
        }
        Ok(Self {
            household_name,
            ..self
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationPolicy {
    Conservative,
    Balanced,
    Assistant,
}

impl AutomationPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Balanced => "balanced",
            Self::Assistant => "assistant",
        }
    }
}

impl TryFrom<&str> for AutomationPolicy {
    type Error = ValidationError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "conservative" => Ok(Self::Conservative),
            "balanced" => Ok(Self::Balanced),
            "assistant" => Ok(Self::Assistant),
            _ => Err(ValidationError::InvalidAutomationPolicy),
        }
    }
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

impl TryFrom<&str> for Theme {
    type Error = ValidationError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "system" => Ok(Self::System),
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(ValidationError::InvalidTheme),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("household name must contain 1 to 80 characters")]
    InvalidHouseholdName,
    #[error("theme is not supported")]
    InvalidTheme,
    #[error("automation policy is not supported")]
    InvalidAutomationPolicy,
    #[error("quiet hours must contain two different valid times")]
    InvalidQuietHours,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trims_valid_name() {
        let value = Settings {
            household_name: "  Home  ".into(),
            ..Settings::default()
        }
        .validate()
        .unwrap();
        assert_eq!(value.household_name, "Home");
    }
    #[test]
    fn rejects_blank_name() {
        assert_eq!(
            Settings {
                household_name: "   ".into(),
                ..Settings::default()
            }
            .validate(),
            Err(ValidationError::InvalidHouseholdName)
        );
    }

    #[test]
    fn balanced_is_the_default_automation_policy() {
        assert_eq!(
            Settings::default().automation_policy,
            AutomationPolicy::Balanced
        );
        assert_eq!(
            AutomationPolicy::try_from("assistant").unwrap(),
            AutomationPolicy::Assistant
        );
        assert_eq!(
            AutomationPolicy::try_from("unbounded"),
            Err(ValidationError::InvalidAutomationPolicy)
        );
    }

    #[test]
    fn notification_preferences_default_off_and_validate_quiet_hours() {
        let settings = Settings::default();
        assert!(!settings.urgent_alerts_enabled);
        assert!(!settings.notification_delivery_enabled);
        assert!(!settings.appointment_reminders_enabled);
        assert!(!settings.morning_summary_enabled);
        assert!(!settings.evening_summary_enabled);
        assert_eq!(settings.quiet_hours_start_minute, 1320);
        assert_eq!(settings.quiet_hours_end_minute, 420);
        assert_eq!(
            Settings {
                quiet_hours_end_minute: 1320,
                ..settings
            }
            .validate(),
            Err(ValidationError::InvalidQuietHours)
        );
    }
}
