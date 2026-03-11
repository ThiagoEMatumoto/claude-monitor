use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::analytics;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
}

/// Default pricing table (Claude 4 family, as of 2025).
pub fn default_pricing() -> HashMap<String, ModelPricing> {
    let mut m = HashMap::new();
    m.insert(
        "opus".to_string(),
        ModelPricing {
            input_per_mtok: 15.0,
            output_per_mtok: 75.0,
            cache_read_per_mtok: 1.50,
            cache_write_per_mtok: 18.75,
        },
    );
    m.insert(
        "sonnet".to_string(),
        ModelPricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.30,
            cache_write_per_mtok: 3.75,
        },
    );
    m.insert(
        "haiku".to_string(),
        ModelPricing {
            input_per_mtok: 0.80,
            output_per_mtok: 4.0,
            cache_read_per_mtok: 0.08,
            cache_write_per_mtok: 1.0,
        },
    );
    m
}

/// Categorize a model string (e.g. "claude-sonnet-4-20250514") into a pricing tier.
pub fn model_tier(model: &str) -> &str {
    let lower = model.to_lowercase();
    if lower.contains("opus") {
        "opus"
    } else if lower.contains("haiku") {
        "haiku"
    } else {
        "sonnet"
    }
}

fn calculate_cost(
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    pricing: &ModelPricing,
) -> f64 {
    let mtok = 1_000_000.0;
    (input_tokens as f64 / mtok) * pricing.input_per_mtok
        + (output_tokens as f64 / mtok) * pricing.output_per_mtok
        + (cache_read_tokens as f64 / mtok) * pricing.cache_read_per_mtok
        + (cache_write_tokens as f64 / mtok) * pricing.cache_write_per_mtok
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCost {
    pub tier: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostSummary {
    pub total_cost_usd: f64,
    pub by_model: Vec<ModelCost>,
}

/// Calculate cost from analytics data for a given time range.
pub fn get_cost_summary(hours: u64) -> CostSummary {
    let analytics = analytics::get_session_analytics(hours);
    let pricing = default_pricing();

    // Aggregate tokens per model tier across all sessions
    let mut tier_tokens: HashMap<String, (u64, u64, u64, u64)> = HashMap::new();

    for session in &analytics.sessions {
        for mu in &session.model_usage {
            let tier = model_tier(&mu.model).to_string();
            let entry = tier_tokens.entry(tier).or_default();
            entry.0 += mu.input_tokens;
            entry.1 += mu.output_tokens;
            entry.2 += mu.cache_read_tokens;
            entry.3 += mu.cache_creation_tokens;
        }
    }

    let mut by_model = Vec::new();
    let mut total = 0.0;

    for (tier, (input, output, cache_read, cache_write)) in &tier_tokens {
        let p = pricing
            .get(tier.as_str())
            .unwrap_or_else(|| pricing.get("sonnet").unwrap());
        let cost = calculate_cost(*input, *output, *cache_read, *cache_write, p);
        total += cost;
        by_model.push(ModelCost {
            tier: tier.clone(),
            input_tokens: *input,
            output_tokens: *output,
            cache_read_tokens: *cache_read,
            cache_write_tokens: *cache_write,
            cost_usd: cost,
        });
    }

    by_model.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    CostSummary {
        total_cost_usd: total,
        by_model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_tier() {
        assert_eq!(model_tier("claude-opus-4-20250514"), "opus");
        assert_eq!(model_tier("claude-sonnet-4-20250514"), "sonnet");
        assert_eq!(model_tier("claude-haiku-4-5-20251001"), "haiku");
        assert_eq!(model_tier("unknown-model"), "sonnet");
    }

    #[test]
    fn test_calculate_cost() {
        let pricing = ModelPricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.30,
            cache_write_per_mtok: 3.75,
        };
        // 1M input tokens = $3.00
        let cost = calculate_cost(1_000_000, 0, 0, 0, &pricing);
        assert!((cost - 3.0).abs() < 0.001);

        // 1M output tokens = $15.00
        let cost = calculate_cost(0, 1_000_000, 0, 0, &pricing);
        assert!((cost - 15.0).abs() < 0.001);

        // Mixed: 500K input + 100K output + 200K cache read + 50K cache write
        let cost = calculate_cost(500_000, 100_000, 200_000, 50_000, &pricing);
        let expected = 0.5 * 3.0 + 0.1 * 15.0 + 0.2 * 0.30 + 0.05 * 3.75;
        assert!((cost - expected).abs() < 0.001);
    }

    #[test]
    fn test_default_pricing_has_all_tiers() {
        let p = default_pricing();
        assert!(p.contains_key("opus"));
        assert!(p.contains_key("sonnet"));
        assert!(p.contains_key("haiku"));
    }
}
