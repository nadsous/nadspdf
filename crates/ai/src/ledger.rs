//! What the assistants used, month by month, kept with the settings so the user always knows
//! what they spend.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Usage;

/// One month's totals.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonthTotal {
    pub requests: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The known cost in US dollars: the provider's figures, or the user's prices.
    pub cost_usd: f64,
    /// Requests whose cost could not be known (no prices entered).
    pub unpriced: u64,
}

/// Totals by month (`2026-11`). At most the last 24 months are kept.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ledger {
    pub months: BTreeMap<String, MonthTotal>,
}

impl Ledger {
    /// Count one request made in `month`, with its `cost` when known.
    pub fn record(&mut self, month: &str, usage: &Usage, cost: Option<f64>) {
        let m = self.months.entry(month.to_string()).or_default();
        m.requests = m.requests.saturating_add(1);
        m.input_tokens = m.input_tokens.saturating_add(usage.input_tokens);
        m.output_tokens = m.output_tokens.saturating_add(usage.output_tokens);
        match cost.filter(|c| c.is_finite() && *c >= 0.0) {
            Some(c) => m.cost_usd += c,
            None => m.unpriced = m.unpriced.saturating_add(1),
        }
        while self.months.len() > 24 {
            let Some(first) = self.months.keys().next().cloned() else { break };
            self.months.remove(&first);
        }
    }

    pub fn month(&self, month: &str) -> MonthTotal {
        self.months.get(month).cloned().unwrap_or_default()
    }
}

/// The month (`YYYY-MM`, UTC) of Unix time `secs`.
pub fn month_of(secs: u64) -> String {
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_add_up_and_old_ones_go() {
        let mut l = Ledger::default();
        let u = Usage { input_tokens: 1000, output_tokens: 200, cost_usd: None };
        l.record("2026-11", &u, Some(0.01));
        l.record("2026-11", &u, None);
        let m = l.month("2026-11");
        assert_eq!((m.requests, m.input_tokens, m.output_tokens, m.unpriced), (2, 2000, 400, 1));
        assert!((m.cost_usd - 0.01).abs() < 1e-12);
        for i in 0..30 {
            l.record(&format!("2030-{:02}", i % 12 + 1), &u, Some(0.0));
            l.record(&format!("20{:02}-01", 31 + i), &u, Some(0.0));
        }
        assert!(l.months.len() <= 24);
    }

    #[test]
    fn unix_times_give_their_month() {
        assert_eq!(month_of(0), "1970-01");
        assert_eq!(month_of(1_793_836_800), "2026-11"); // 2026-11-05
        assert_eq!(month_of(951_782_400), "2000-02"); // 2000-02-29
    }
}
