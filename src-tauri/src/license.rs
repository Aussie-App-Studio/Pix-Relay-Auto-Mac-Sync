use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::db::{now_ms, Database};
use crate::error::{AppError, AppResult};

const TRIAL_DURATION_MS: i64 = 7 * 86_400_000;

/// Toggle to `true` when transitioning from free GitHub Releases to paid website license keys.
pub const REQUIRE_PAID_LICENSE: bool = false;

/// Update this single URL when launching the dedicated Pix Relay website.
pub const WEBSITE_URL: &str = "https://github.com/Aussie-App-Studio/Pix-Relay-Auto-Mac-Sync";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LicenseMode {
    TrialActive,
    TrialExpired,
    Unlocked,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MacLicenseStatus {
    pub mode: LicenseMode,
    pub requires_license: bool,
    pub can_sync: bool,
    pub days_remaining: i64,
    pub expires_at_ms: Option<i64>,
    pub dev_override: Option<String>,
    pub status_label: String,
    pub website_url: String,
    pub has_license_key: bool,
}

pub struct LicenseService {
    dev_override: Mutex<Option<String>>,
}

impl LicenseService {
    pub fn new() -> Self {
        Self {
            dev_override: Mutex::new(None),
        }
    }

    pub fn set_dev_override(&self, value: Option<String>) {
        if cfg!(debug_assertions) {
            *self.dev_override.lock().unwrap() = value.filter(|s| !s.trim().is_empty());
        }
    }

    pub fn get_status(&self, db: &Database) -> AppResult<MacLicenseStatus> {
        let settings = db.get_settings()?;
        let has_key = settings
            .license_key
            .as_deref()
            .map(is_valid_license_key_format)
            .unwrap_or(false);

        let override_val = if cfg!(debug_assertions) {
            self.dev_override.lock().unwrap().clone()
        } else {
            None
        };

        if let Some(ref ov) = override_val {
            return Ok(match ov.as_str() {
                "trial_expired" => MacLicenseStatus {
                    mode: LicenseMode::TrialExpired,
                    requires_license: true,
                    can_sync: false,
                    days_remaining: 0,
                    expires_at_ms: Some(now_ms() - 3600_000),
                    dev_override: override_val,
                    status_label: "Trial expired — Enter license key to continue syncing".to_string(),
                    website_url: WEBSITE_URL.to_string(),
                    has_license_key: has_key,
                },
                "unlocked" => MacLicenseStatus {
                    mode: LicenseMode::Unlocked,
                    requires_license: REQUIRE_PAID_LICENSE,
                    can_sync: true,
                    days_remaining: 0,
                    expires_at_ms: None,
                    dev_override: override_val,
                    status_label: "Full License Unlocked".to_string(),
                    website_url: WEBSITE_URL.to_string(),
                    has_license_key: true,
                },
                _ => {
                    let exp = now_ms() + 5 * 86_400_000;
                    MacLicenseStatus {
                        mode: LicenseMode::TrialActive,
                        requires_license: true,
                        can_sync: true,
                        days_remaining: 5,
                        expires_at_ms: Some(exp),
                        dev_override: override_val,
                        status_label: "Free Trial · 5 days left".to_string(),
                        website_url: WEBSITE_URL.to_string(),
                        has_license_key: has_key,
                    }
                }
            });
        }

        if !REQUIRE_PAID_LICENSE {
            return Ok(MacLicenseStatus {
                mode: LicenseMode::Unlocked,
                requires_license: false,
                can_sync: true,
                days_remaining: 0,
                expires_at_ms: None,
                dev_override: None,
                status_label: "Direct Mac Edition · Unlocked".to_string(),
                website_url: WEBSITE_URL.to_string(),
                has_license_key: has_key,
            });
        }

        if has_key {
            return Ok(MacLicenseStatus {
                mode: LicenseMode::Unlocked,
                requires_license: true,
                can_sync: true,
                days_remaining: 0,
                expires_at_ms: None,
                dev_override: None,
                status_label: "Full License Unlocked".to_string(),
                website_url: WEBSITE_URL.to_string(),
                has_license_key: true,
            });
        }

        let started = db.get_or_init_trial_started_ms()?;
        let expires = started + TRIAL_DURATION_MS;
        let now = now_ms();
        let remaining_ms = expires - now;
        if remaining_ms > 0 {
            let days = ((remaining_ms + 86_399_999) / 86_400_000).max(1);
            Ok(MacLicenseStatus {
                mode: LicenseMode::TrialActive,
                requires_license: true,
                can_sync: true,
                days_remaining: days,
                expires_at_ms: Some(expires),
                dev_override: None,
                status_label: format!(
                    "Free Trial · {days} {} left",
                    if days == 1 { "day" } else { "days" }
                ),
                website_url: WEBSITE_URL.to_string(),
                has_license_key: false,
            })
        } else {
            Ok(MacLicenseStatus {
                mode: LicenseMode::TrialExpired,
                requires_license: true,
                can_sync: false,
                days_remaining: 0,
                expires_at_ms: Some(expires),
                dev_override: None,
                status_label: "Trial expired — Enter license key to continue syncing".to_string(),
                website_url: WEBSITE_URL.to_string(),
                has_license_key: false,
            })
        }
    }

    pub fn activate_license_key(&self, db: &Database, raw_key: &str) -> AppResult<MacLicenseStatus> {
        let clean = raw_key.trim().to_uppercase();
        if !is_valid_license_key_format(&clean) {
            return Err(AppError::Message(
                "Enter a valid Pix Relay license key (format: PIXM-XXXX-XXXX-XXXX).".to_string(),
            ));
        }
        db.set_license_key(Some(&clean))?;
        db.append_activity("LICENSE", "Activated Mac license key")?;
        self.get_status(db)
    }

    pub fn clear_license_key(&self, db: &Database) -> AppResult<MacLicenseStatus> {
        db.set_license_key(None)?;
        self.get_status(db)
    }
}

pub fn is_valid_license_key_format(raw: &str) -> bool {
    let trimmed = raw.trim().to_uppercase();
    let parts: Vec<&str> = trimmed.split('-').collect();
    parts.len() == 4
        && parts[0] == "PIXM"
        && parts[1..]
            .iter()
            .all(|seg| seg.len() == 4 && seg.chars().all(|c| c.is_ascii_alphanumeric()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_key_validation_works() {
        assert!(is_valid_license_key_format("PIXM-AB12-CD34-EF56"));
        assert!(is_valid_license_key_format("  pixm-1111-2222-3333  "));
        assert!(!is_valid_license_key_format("INVALID-KEY"));
        assert!(!is_valid_license_key_format("PIXM-123-456-789"));
    }
}
