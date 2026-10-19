use runtime_extension_sdk::AuthenticationResponse;
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, Date, Month, OffsetDateTime, UtcOffset};

use crate::config::Config;

pub(crate) fn evaluate(config: &Config, profile: &Value, now_unix: i64) -> AuthenticationResponse {
    let deny = |code: &str| AuthenticationResponse::RegistrationEligibility {
        allowed: false,
        reason_code: Some(code.into()),
    };
    if !config.self_registration_enabled {
        return deny("github_registration_disabled");
    }
    let Some(created) = profile
        .get("created_at")
        .and_then(Value::as_str)
        .and_then(|s| OffsetDateTime::parse(s, &Rfc3339).ok())
    else {
        return deny("github_account_created_at_invalid");
    };
    let Ok(now) = OffsetDateTime::from_unix_timestamp(now_unix) else {
        return deny("github_account_created_at_invalid");
    };
    if created > now {
        return deny("github_account_created_at_invalid");
    }
    if config.minimum_account_age_years == 0 {
        return AuthenticationResponse::RegistrationEligibility {
            allowed: true,
            reason_code: None,
        };
    }
    let created = created.to_offset(UtcOffset::UTC).date();
    let anniversary = i32::try_from(config.minimum_account_age_years)
        .ok()
        .and_then(|years| created.year().checked_add(years))
        .and_then(|year| {
            // A February 29 account reaches its anniversary on February 28 in a non-leap year.
            Date::from_calendar_date(year, created.month(), created.day())
                .ok()
                .or_else(|| {
                    (created.month() == Month::February && created.day() == 29)
                        .then(|| Date::from_calendar_date(year, Month::February, 28).ok())
                        .flatten()
                })
        });
    if anniversary.is_none_or(|day| now.date() < day) {
        return deny("github_account_too_new");
    }
    AuthenticationResponse::RegistrationEligibility {
        allowed: true,
        reason_code: None,
    }
}
