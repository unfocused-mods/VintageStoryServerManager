use crate::domain::*;

pub fn recommend(input: RecommendationInput) -> Recommendations {
    let groups: std::collections::HashSet<_> = input
        .samples
        .iter()
        .map(|s| (s.location.clone(), s.players))
        .collect();
    let mut candidates: Vec<RankedQuote> = input
        .quotes
        .into_iter()
        .map(|q| {
            let total = q.total();
            let mut reasons = vec![];
            if q.provider == Provider::Azure {
                let family = q.size.trim_start_matches("Standard_");
                if !(family.starts_with('D') || family.starts_with('F')) {
                    reasons.push(
                        "Automatic Azure recommendations use nonburstable D/F families".into(),
                    );
                }
            }
            if !q.available {
                reasons.push("Capacity not confirmed".into());
            }
            if q.memory_gib < input.min_memory_gib || q.vcpus < input.min_vcpus {
                reasons.push("Below requested resources".into());
            }
            if q.currency != input.currency {
                reasons.push("Currency differs; conversion not available".into());
            }
            if total.is_none() {
                reasons.push("Incomplete cost estimate".into());
            }
            if total.zip(input.budget).is_some_and(|(t, b)| t > b) {
                reasons.push("Above budget".into());
            }
            let fresh = chrono::DateTime::parse_from_rfc3339(&q.quoted_at)
                .ok()
                .is_some_and(|t| {
                    let age = chrono::Utc::now().signed_duration_since(t);
                    age.num_seconds() >= 0 && age.num_hours() < 24
                });
            if !fresh {
                reasons.push("Quote is stale; refresh before deployment".into());
            }
            let samples: Vec<_> = input
                .samples
                .iter()
                .filter(|s| {
                    s.quote_id == q.id
                        && s.players > 0
                        && s.milliseconds.is_finite()
                        && s.milliseconds >= 0.0
                        && matches!(s.source.as_str(), "measured" | "geographic_estimate")
                })
                .collect();
            let players: u64 = samples.iter().map(|s| s.players as u64).sum();
            let present: std::collections::HashSet<_> = samples
                .iter()
                .map(|s| (s.location.clone(), s.players))
                .collect();
            let weighted_ms = (players > 0 && present == groups && present.len() == samples.len())
                .then(|| {
                    samples
                        .iter()
                        .map(|s| s.milliseconds * s.players as f64)
                        .sum::<f64>()
                        / players as f64
                });
            let worst_ms = samples.iter().map(|s| s.milliseconds).reduce(f64::max);
            RankedQuote {
                quote: q,
                total,
                weighted_ms,
                worst_ms,
                eligible: reasons.is_empty(),
                reasons,
            }
        })
        .collect();
    candidates.sort_by(|a, b| {
        a.total
            .unwrap_or(f64::INFINITY)
            .total_cmp(&b.total.unwrap_or(f64::INFINITY))
    });
    let eligible: Vec<_> = candidates.iter().filter(|q| q.eligible).collect();
    let cheapest = eligible.first().map(|q| q.quote.id.clone());
    let best = eligible
        .iter()
        .filter(|q| q.weighted_ms.is_some())
        .min_by(|a, b| a.weighted_ms.unwrap().total_cmp(&b.weighted_ms.unwrap()));
    let lowest_latency = best.map(|q| q.quote.id.clone());
    let balanced = best
        .and_then(|b| {
            eligible.iter().find(|q| {
                q.weighted_ms
                    .is_some_and(|ms| ms <= b.weighted_ms.unwrap() + 30.0)
            })
        })
        .map(|q| q.quote.id.clone());
    Recommendations {
        balanced,
        cheapest,
        lowest_latency,
        candidates,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn quote(id: &str, cost: f64) -> Quote {
        Quote {
            id: id.into(),
            provider: Provider::Azure,
            region: "eastus".into(),
            size: "D2s_v5".into(),
            vcpus: 2,
            memory_gib: 8.0,
            currency: "USD".into(),
            monthly_compute: cost,
            monthly_storage: Some(0.0),
            monthly_ip: Some(0.0),
            monthly_transfer: Some(0.0),
            monthly_backup: Some(0.0),
            quoted_at: now(),
            available: true,
            shared_cpu: false,
            assumptions: vec![],
        }
    }
    #[test]
    fn balances_cost_within_latency_window_and_excludes_unknown() {
        let r = recommend(RecommendationInput {
            quotes: vec![
                quote("unknown", 1.0),
                quote("slow", 5.0),
                quote("balanced", 10.0),
                quote("fast", 20.0),
            ],
            samples: vec![("slow", 100.0), ("balanced", 45.0), ("fast", 20.0)]
                .into_iter()
                .map(|(id, ms)| LatencySample {
                    quote_id: id.into(),
                    location: "home".into(),
                    players: 2,
                    milliseconds: ms,
                    source: "measured".into(),
                })
                .collect(),
            min_memory_gib: 4.0,
            min_vcpus: 2,
            budget: None,
            currency: "USD".into(),
        });
        assert_eq!(r.balanced.as_deref(), Some("balanced"));
        assert_eq!(r.cheapest.as_deref(), Some("unknown"));
    }
    #[test]
    fn incomplete_prices_cannot_win() {
        let mut q = quote("bad", 1.0);
        q.monthly_ip = None;
        let r = recommend(RecommendationInput {
            quotes: vec![q],
            samples: vec![],
            min_memory_gib: 4.0,
            min_vcpus: 2,
            budget: None,
            currency: "USD".into(),
        });
        assert!(r.cheapest.is_none());
    }
    #[test]
    fn unsuitable_azure_families_stale_quotes_and_mixed_currencies_are_excluded() {
        let mut burst = quote("burst", 1.0);
        burst.size = "Standard_B2s".into();
        let mut stale = quote("stale", 2.0);
        stale.quoted_at = "2020-01-01T00:00:00Z".into();
        let mut euro = quote("euro", 3.0);
        euro.currency = "EUR".into();
        let result = recommend(RecommendationInput {
            quotes: vec![burst, stale, euro, quote("suitable", 10.0)],
            samples: vec![],
            min_memory_gib: 4.0,
            min_vcpus: 2,
            budget: None,
            currency: "USD".into(),
        });
        assert_eq!(result.cheapest.as_deref(), Some("suitable"));
        assert!(result.balanced.is_none());
    }
}
