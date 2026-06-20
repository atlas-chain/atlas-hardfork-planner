use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::time::sleep;

use crate::quantity::parse_quantity;
use crate::store::ScheduleStore;

pub async fn poll_loop(
    store: Arc<ScheduleStore>,
    client: reqwest::Client,
    rpc_url: String,
    timeout: Duration,
    interval: Duration,
) {
    loop {
        match fetch_block_number(&client, &rpc_url, timeout).await {
            Ok(block) => {
                if store.set_current_block(block) {
                    let snapshot = store.snapshot();
                    println!(
                        "{}",
                        json!({
                            "message": "current block updated",
                            "block": block.to_string(),
                            "version": snapshot.version,
                            "chainId": snapshot.chain_id,
                            "hash": snapshot.hash,
                        })
                    );
                }
            }
            Err(message) => {
                eprintln!("{}", json!({ "message": "rpc poll failed", "error": message }));
            }
        }
        sleep(interval).await;
    }
}

pub async fn fetch_block_number(
    client: &reqwest::Client,
    rpc_url: &str,
    timeout: Duration,
) -> Result<u64, String> {
    let request_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_blockNumber",
        "params": [],
    });

    let response = tokio::time::timeout(timeout, client.post(rpc_url).json(&request_body).send())
        .await
        .map_err(|_| "rpc request timed out".to_string())?
        .map_err(|error| format!("rpc request failed: {error}"))?;

    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|error| format!("rpc decode failed: {error}"))?;

    if !status.is_success() {
        return Err(format!("rpc http status {status}"));
    }

    if let Some(error_value) = value.get("error").filter(|v| !v.is_null()) {
        return Err(format!("rpc error: {error_value}"));
    }

    let result = value
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| "rpc response missing string result".to_string())?;

    parse_quantity(result)
        .ok_or_else(|| format!("rpc returned non-quantity block number: {result}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ScheduleDocument, ScheduleEntry};

    fn entry() -> ScheduleEntry {
        ScheduleEntry {
            activation_block: 0,
            min_base_fee_per_gas: "440000000".to_string(),
            elasticity_multiplier: 2,
            base_fee_max_change_denominator: 8,
            max_block_gas_limit: "30000000".to_string(),
        }
    }

    #[test]
    fn parse_quantity_reads_eth_block_number_result() {
        assert_eq!(parse_quantity("0x10"), Some(16));
        assert_eq!(parse_quantity("0x1e8480"), Some(2_000_000));
    }

    #[test]
    fn document_with_current_block_serializes_for_gating() {
        let doc = ScheduleDocument {
            chain_id: 42069,
            version: 1,
            current_block: Some(2_000_000),
            schedule: vec![entry()],
        };
        let serialized = crate::model::canonicalize(&doc);
        assert!(serialized.contains("\"currentBlock\": 2000000"));
    }
}
