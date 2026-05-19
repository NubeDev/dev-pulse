//! Parse GitHub's rate-limit headers in one place and convert the
//! mess of conditions (primary remaining-quota, secondary rate
//! limits, `Retry-After`, raw 429) into a single typed signal the
//! [`Client`](super::Client) acts on.
//!
//! Reference: <https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api>
//!
//! TODO §Phase-2 explicitly says "Octocrab client wrapper with
//! rate-limit pacing in **one** place." This module is that place.

use chrono::{DateTime, TimeZone, Utc};
use http::HeaderMap;

/// What the rate-limit headers on a single response are telling us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitSignal {
    /// We're under the budget. Proceed.
    Ok {
        /// Remaining primary-budget requests in the current window.
        /// Useful for telemetry; the wrapper does not pre-emptively
        /// throttle on this number.
        remaining: u64,
        /// When the primary window resets, as UTC. We pause here
        /// only if a *subsequent* 403/429 confirms we hit the cap.
        reset_at: DateTime<Utc>,
    },
    /// We tripped the primary quota (status 403 with
    /// `x-ratelimit-remaining: 0`) — sleep until reset.
    PrimaryExhausted {
        /// UTC moment the quota window resets.
        reset_at: DateTime<Utc>,
    },
    /// GitHub flagged a *secondary* rate limit (abuse detection).
    /// `Retry-After` is authoritative; absent that, we fall back to
    /// a documented 60s minimum.
    SecondaryRateLimit {
        /// Wall-clock moment we may retry.
        retry_at: DateTime<Utc>,
    },
}

/// Interpret rate-limit-relevant headers on a response. `status`
/// disambiguates a 200 with `remaining=0` (only a *warning*) from a
/// 403/429 that *enforces* the cap.
pub fn classify(status: u16, headers: &HeaderMap, now: DateTime<Utc>) -> Option<RateLimitSignal> {
    let remaining = header_u64(headers, "x-ratelimit-remaining");
    let reset = header_unix(headers, "x-ratelimit-reset");

    // GitHub uses both "x-ratelimit-resource" and a plaintext body
    // marker to indicate secondary limits; the header is enough for
    // us to disambiguate from a primary 403.
    let resource = headers
        .get("x-ratelimit-resource")
        .and_then(|v| v.to_str().ok());

    if status == 429
        || (status == 403 && (resource == Some("secondary") || headers.contains_key("retry-after")))
    {
        let retry_at = match header_u64(headers, "retry-after") {
            Some(secs) => now + chrono::Duration::seconds(secs as i64),
            None => now + chrono::Duration::seconds(60), // documented secondary-RL minimum
        };
        return Some(RateLimitSignal::SecondaryRateLimit { retry_at });
    }

    if status == 403 && remaining == Some(0) {
        return reset.map(|reset_at| RateLimitSignal::PrimaryExhausted { reset_at });
    }

    match (remaining, reset) {
        (Some(remaining), Some(reset_at)) => Some(RateLimitSignal::Ok { remaining, reset_at }),
        _ => None,
    }
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}

fn header_unix(headers: &HeaderMap, name: &str) -> Option<DateTime<Utc>> {
    let secs = header_u64(headers, name)?;
    Utc.timestamp_opt(secs as i64, 0).single()
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    fn hdrs(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut m = HeaderMap::new();
        for (k, v) in pairs {
            m.insert(
                http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        m
    }

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000, 0).single().unwrap()
    }

    #[test]
    fn ok_path_returns_remaining_and_reset() {
        let signal = classify(
            200,
            &hdrs(&[
                ("x-ratelimit-remaining", "4321"),
                ("x-ratelimit-reset", "1700001000"),
            ]),
            now(),
        );
        match signal {
            Some(RateLimitSignal::Ok { remaining, reset_at }) => {
                assert_eq!(remaining, 4321);
                assert_eq!(reset_at.timestamp(), 1_700_001_000);
            }
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn primary_exhausted_on_403_with_remaining_zero() {
        let signal = classify(
            403,
            &hdrs(&[
                ("x-ratelimit-remaining", "0"),
                ("x-ratelimit-reset", "1700001000"),
            ]),
            now(),
        );
        assert!(matches!(
            signal,
            Some(RateLimitSignal::PrimaryExhausted { .. })
        ));
    }

    #[test]
    fn secondary_rl_via_resource_header_uses_retry_after() {
        let signal = classify(
            403,
            &hdrs(&[
                ("x-ratelimit-resource", "secondary"),
                ("retry-after", "42"),
            ]),
            now(),
        );
        match signal {
            Some(RateLimitSignal::SecondaryRateLimit { retry_at }) => {
                assert_eq!((retry_at - now()).num_seconds(), 42);
            }
            other => panic!("expected SecondaryRateLimit, got {other:?}"),
        }
    }

    #[test]
    fn secondary_rl_falls_back_to_60s_when_retry_after_absent() {
        let signal = classify(
            403,
            &hdrs(&[("x-ratelimit-resource", "secondary")]),
            now(),
        );
        match signal {
            Some(RateLimitSignal::SecondaryRateLimit { retry_at }) => {
                assert_eq!((retry_at - now()).num_seconds(), 60);
            }
            other => panic!("expected 60s fallback, got {other:?}"),
        }
    }

    #[test]
    fn raw_429_is_secondary_rate_limit() {
        let signal = classify(429, &hdrs(&[("retry-after", "5")]), now());
        match signal {
            Some(RateLimitSignal::SecondaryRateLimit { retry_at }) => {
                assert_eq!((retry_at - now()).num_seconds(), 5);
            }
            other => panic!("expected SecondaryRateLimit for 429, got {other:?}"),
        }
    }

    #[test]
    fn missing_headers_returns_none() {
        assert!(classify(200, &hdrs(&[]), now()).is_none());
    }
}
