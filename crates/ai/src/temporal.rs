use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime, Timelike, Weekday};
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TemporalPrecision {
    Date,
    DateTime,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TemporalRelation {
    Exact,
    Relative,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTemporal {
    pub source_expression: String,
    pub local_date: String,
    pub local_time: Option<String>,
    pub utc_offset_seconds: i32,
    pub precision: TemporalPrecision,
    pub relation: TemporalRelation,
}

pub fn resolve_temporal_expression(
    expression: &str,
    reference_at: &str,
) -> Result<ResolvedTemporal, TemporalError> {
    let reference: DateTime<FixedOffset> =
        DateTime::parse_from_rfc3339(reference_at).map_err(|_| TemporalError::InvalidReference)?;
    let normalized = expression.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(TemporalError::Empty);
    }

    let (date, relation) = if let Some(date) = explicit_date(&normalized) {
        (date?, TemporalRelation::Exact)
    } else if normalized
        .split(non_alphanumeric)
        .any(|word| word == "today")
    {
        (reference.date_naive(), TemporalRelation::Relative)
    } else if normalized
        .split(non_alphanumeric)
        .any(|word| word == "tomorrow")
    {
        (
            reference.date_naive() + Duration::days(1),
            TemporalRelation::Relative,
        )
    } else if let Some(weekday) = find_weekday(&normalized) {
        (
            next_weekday(reference.date_naive(), weekday),
            TemporalRelation::Relative,
        )
    } else {
        return Err(TemporalError::Ambiguous);
    };

    let time = find_time(&normalized)?;
    Ok(ResolvedTemporal {
        source_expression: expression.trim().to_owned(),
        local_date: date.format("%Y-%m-%d").to_string(),
        local_time: time.map(|value| format!("{:02}:{:02}", value.hour(), value.minute())),
        utc_offset_seconds: reference.offset().local_minus_utc(),
        precision: if time.is_some() {
            TemporalPrecision::DateTime
        } else {
            TemporalPrecision::Date
        },
        relation,
    })
}

fn explicit_date(value: &str) -> Option<Result<NaiveDate, TemporalError>> {
    value
        .split(non_date_character)
        .find(|token| token.len() == 10 && token.as_bytes().get(4) == Some(&b'-'))
        .map(|token| {
            NaiveDate::parse_from_str(token, "%Y-%m-%d").map_err(|_| TemporalError::InvalidDate)
        })
}

fn find_weekday(value: &str) -> Option<Weekday> {
    value.split(non_alphanumeric).find_map(|word| match word {
        "monday" => Some(Weekday::Mon),
        "tuesday" => Some(Weekday::Tue),
        "wednesday" => Some(Weekday::Wed),
        "thursday" => Some(Weekday::Thu),
        "friday" => Some(Weekday::Fri),
        "saturday" => Some(Weekday::Sat),
        "sunday" => Some(Weekday::Sun),
        _ => None,
    })
}

fn next_weekday(reference: NaiveDate, target: Weekday) -> NaiveDate {
    let current = i64::from(reference.weekday().num_days_from_monday());
    let target = i64::from(target.num_days_from_monday());
    reference + Duration::days((target - current).rem_euclid(7))
}

fn find_time(value: &str) -> Result<Option<NaiveTime>, TemporalError> {
    for token in value.split(|character: char| {
        character.is_whitespace() || matches!(character, ',' | ';' | '(' | ')')
    }) {
        let token = token.trim_matches(|character: char| matches!(character, '.' | '!' | '?'));
        if let Some(time) = parse_time_token(token)? {
            return Ok(Some(time));
        }
    }
    Ok(None)
}

fn parse_time_token(token: &str) -> Result<Option<NaiveTime>, TemporalError> {
    let (clock, meridiem) = if let Some(clock) = token.strip_suffix("am") {
        (clock, Some(false))
    } else if let Some(clock) = token.strip_suffix("pm") {
        (clock, Some(true))
    } else {
        (token, None)
    };
    if !clock.chars().any(|character| character.is_ascii_digit()) {
        return Ok(None);
    }
    if meridiem.is_none() && !clock.contains(':') {
        return Ok(None);
    }
    let mut parts = clock.split(':');
    let mut hour: u32 = parts
        .next()
        .ok_or(TemporalError::InvalidTime)?
        .parse()
        .map_err(|_| TemporalError::InvalidTime)?;
    let minute: u32 = parts
        .next()
        .map(str::parse)
        .transpose()
        .map_err(|_| TemporalError::InvalidTime)?
        .unwrap_or(0);
    if parts.next().is_some() {
        return Err(TemporalError::InvalidTime);
    }
    if let Some(is_pm) = meridiem {
        if !(1..=12).contains(&hour) {
            return Err(TemporalError::InvalidTime);
        }
        hour = match (hour, is_pm) {
            (12, false) => 0,
            (12, true) => 12,
            (_, true) => hour + 12,
            _ => hour,
        };
    }
    NaiveTime::from_hms_opt(hour, minute, 0)
        .map(Some)
        .ok_or(TemporalError::InvalidTime)
}

fn non_alphanumeric(character: char) -> bool {
    !character.is_ascii_alphanumeric()
}

fn non_date_character(character: char) -> bool {
    !(character.is_ascii_digit() || character == '-')
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TemporalError {
    #[error("temporal expression is empty")]
    Empty,
    #[error("reference timestamp must be RFC 3339 with an explicit offset")]
    InvalidReference,
    #[error("temporal expression contains an invalid date")]
    InvalidDate,
    #[error("temporal expression contains an invalid time")]
    InvalidTime,
    #[error("temporal expression is ambiguous and requires review")]
    Ambiguous,
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE: &str = "2026-08-13T09:30:00+01:00";

    #[test]
    fn resolves_tomorrow_with_twelve_hour_time() {
        let resolved = resolve_temporal_expression("tomorrow at 1pm", REFERENCE).unwrap();
        assert_eq!(resolved.local_date, "2026-08-14");
        assert_eq!(resolved.local_time.as_deref(), Some("13:00"));
        assert_eq!(resolved.relation, TemporalRelation::Relative);
    }

    #[test]
    fn resolves_next_named_weekday_deterministically() {
        let resolved = resolve_temporal_expression("by Wednesday", REFERENCE).unwrap();
        assert_eq!(resolved.local_date, "2026-08-19");
        assert_eq!(resolved.precision, TemporalPrecision::Date);
    }

    #[test]
    fn keeps_same_day_when_named_weekday_matches_reference() {
        let resolved = resolve_temporal_expression("Thursday at 16:30", REFERENCE).unwrap();
        assert_eq!(resolved.local_date, "2026-08-13");
        assert_eq!(resolved.local_time.as_deref(), Some("16:30"));
    }

    #[test]
    fn preserves_explicit_iso_date() {
        let resolved = resolve_temporal_expression("2026-09-02 08:15", REFERENCE).unwrap();
        assert_eq!(resolved.local_date, "2026-09-02");
        assert_eq!(resolved.relation, TemporalRelation::Exact);
    }

    #[test]
    fn rejects_vague_dates_instead_of_guessing() {
        assert_eq!(
            resolve_temporal_expression("sometime next week", REFERENCE),
            Err(TemporalError::Ambiguous)
        );
    }
}
