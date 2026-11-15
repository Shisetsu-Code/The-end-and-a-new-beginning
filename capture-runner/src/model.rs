//! Capture data. Monetary fields are optional native payload, never acceptance criteria.
use anyhow::{Context, Result, ensure};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModeKind {
    #[serde(alias = "bet", alias = "spin")]
    Normal,
    #[serde(alias = "buy", alias = "buy_bonus")]
    Purchase,
    #[serde(rename = "antebet", alias = "ante_bet")]
    AnteBet,
    #[serde(alias = "activator")]
    Booster,
    Variant,
    Special,
}
impl ModeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Purchase => "purchase",
            Self::AnteBet => "antebet",
            Self::Booster => "booster",
            Self::Variant => "variant",
            Self::Special => "special",
        }
    }
    pub fn operation(self) -> &'static str {
        match self {
            Self::Normal => "bet",
            Self::Purchase => "buy_bonus",
            Self::AnteBet => "ante_bet",
            other => other.as_str(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mode {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: ModeKind,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub requires_prerequisites: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub modes: Vec<Mode>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub games: Vec<Game>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Completion {
    pub native_round_id: Option<String>,
    pub details: Value,
}
impl Completion {
    pub fn new(native_round_id: Option<String>) -> Self {
        Self {
            native_round_id,
            details: json!({}),
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    Complete,
    UnknownEvent,
    TransportError,
    ProviderError,
    Unsupported,
    Interrupted,
}
#[derive(Debug)]
pub struct UnknownEvent(pub String);
impl fmt::Display for UnknownEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for UnknownEvent {}
pub fn unknown_event(message: impl Into<String>) -> anyhow::Error {
    UnknownEvent(message.into()).into()
}
#[derive(Debug)]
pub struct TransportError(pub String);
impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for TransportError {}
pub fn transport_error(message: impl Into<String>) -> anyhow::Error {
    TransportError(message.into()).into()
}
pub fn classify_error(error: &anyhow::Error) -> CaptureStatus {
    if error.downcast_ref::<UnknownEvent>().is_some()
        || error.downcast_ref::<serde_json::Error>().is_some()
        || error.downcast_ref::<roxmltree::Error>().is_some()
        || error.downcast_ref::<base64::DecodeError>().is_some()
    {
        CaptureStatus::UnknownEvent
    } else if error.downcast_ref::<TransportError>().is_some() {
        CaptureStatus::TransportError
    } else {
        CaptureStatus::ProviderError
    }
}
pub fn canonical_provider(provider: &str) -> &str {
    match provider {
        "reelkingdom" => "pragmatic",
        other => other,
    }
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
// Exact numeric encoding helpers; no wallet acceptance policy.
pub fn decimal(value: &str) -> Result<Decimal> {
    let value = if let Some((mantissa, exponent)) = value.split_once(['e', 'E']) {
        // Parse the mantissa exactly first: `from_scientific` itself uses a lossy parser.
        Decimal::from_str_exact(mantissa).context("invalid exact scientific mantissa")?;
        let shift: i32 = exponent.parse().context("invalid scientific exponent")?;
        ensure!(
            (-128..=128).contains(&shift),
            "scientific exponent outside supported exact range"
        );
        let (sign, unsigned) = if let Some(v) = mantissa.strip_prefix('-') {
            ("-", v)
        } else if let Some(v) = mantissa.strip_prefix('+') {
            ("", v)
        } else {
            ("", mantissa)
        };
        let unsigned = unsigned.replace('_', "");
        let (integer, fraction) = unsigned.split_once('.').unwrap_or((&unsigned, ""));
        let digits = format!("{integer}{fraction}");
        let point = integer.len() as i32 + shift;
        let expanded = if point <= 0 {
            format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
        } else if point as usize >= digits.len() {
            format!(
                "{sign}{digits}{}",
                "0".repeat(point as usize - digits.len())
            )
        } else {
            format!(
                "{sign}{}.{}",
                &digits[..point as usize],
                &digits[point as usize..]
            )
        };
        Decimal::from_str_exact(&expanded)
            .context("scientific amount cannot be represented exactly")?
    } else {
        Decimal::from_str_exact(value).context("invalid or inexact decimal amount")?
    };
    ensure!(value >= Decimal::ZERO, "negative native money amount");
    Ok(value)
}
fn scaled_mantissa(value: Decimal, scale: u32) -> Option<i128> {
    let factor = 10i128.checked_pow(scale.checked_sub(value.scale())?)?;
    value.mantissa().checked_mul(factor)
}
fn exact_from_parts(mut mantissa: i128, mut scale: u32) -> Result<Decimal> {
    // Remove only zero fractional digits when the Decimal representation requires it.
    while scale > 28 && mantissa % 10 == 0 {
        mantissa /= 10;
        scale -= 1;
    }
    loop {
        match Decimal::try_from_i128_with_scale(mantissa, scale) {
            Ok(value) => return Ok(value),
            Err(_) if scale > 0 && mantissa % 10 == 0 => {
                mantissa /= 10;
                scale -= 1;
            }
            Err(error) => return Err(error).context("exact decimal result not representable"),
        }
    }
}
pub fn exact_add(a: Decimal, b: Decimal) -> Result<Decimal> {
    let scale = a.scale().max(b.scale());
    let x = scaled_mantissa(a, scale).context("exact addition overflow")?;
    let y = scaled_mantissa(b, scale).context("exact addition overflow")?;
    exact_from_parts(x.checked_add(y).context("exact addition overflow")?, scale)
}
pub fn exact_sub(a: Decimal, b: Decimal) -> Result<Decimal> {
    let scale = a.scale().max(b.scale());
    let x = scaled_mantissa(a, scale).context("exact subtraction overflow")?;
    let y = scaled_mantissa(b, scale).context("exact subtraction overflow")?;
    exact_from_parts(
        x.checked_sub(y).context("exact subtraction overflow")?,
        scale,
    )
}
pub fn exact_mul(a: Decimal, b: Decimal) -> Result<Decimal> {
    let n = a
        .mantissa()
        .checked_mul(b.mantissa())
        .context("exact multiplication overflow")?;
    exact_from_parts(n, a.scale() + b.scale())
}
pub fn exact_div100(value: Decimal) -> Result<Decimal> {
    exact_from_parts(value.mantissa(), value.scale() + 2)
}

/// Filesystem component independent of provider punctuation, with a collision-resistant suffix.
pub fn component(value: &str) -> String {
    let prefix: String = value
        .chars()
        .take(48)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "{}-{}",
        if prefix.is_empty() { "item" } else { &prefix },
        &sha256(value.as_bytes())[..12]
    )
}
pub fn check_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 100
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "invalid identifier: {id}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_close_does_not_require_money() {
        let value = serde_json::to_value(Completion::new(Some("r1".into()))).unwrap();
        assert!(value.get("balance").is_none());
    }
    #[test]
    fn novelty_survives_context() {
        let error = unknown_event("new_phase").context("spin");
        assert_eq!(classify_error(&error), CaptureStatus::UnknownEvent);
    }
    #[test]
    fn paths_cannot_escape_or_collide() {
        assert!(!component("../../a").contains('/'));
        assert_ne!(component("a/b"), component("a_b"));
    }
}
