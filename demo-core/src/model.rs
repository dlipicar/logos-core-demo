//! What the node reports, parsed. Pure: no runtime needed to test it.

use serde_json::Value;

/// A `result` reply's payload: blockchain_module puts its info in `value` as a
/// JSON document in a string, and reports failures in `error`.
pub fn result_value(reply: &Value) -> Result<Value, String> {
    let Some(object) = reply.as_object().filter(|o| o.contains_key("success")) else {
        return Ok(reply.clone());
    };
    if !object.get("success").and_then(Value::as_bool).unwrap_or(false) {
        let error = object.get("error").and_then(Value::as_str).unwrap_or("the node refused");
        return Err(error.to_string());
    }
    Ok(match object.get("value") {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone())),
        Some(other) => other.clone(),
        None => Value::Null,
    })
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChainInfo {
    pub mode: String,
    pub height: i64,
    pub slot: i64,
    pub tip: String,
    pub lib_slot: i64,
}

impl ChainInfo {
    pub fn parse(info: &Value) -> Option<ChainInfo> {
        Some(ChainInfo {
            mode: info.get("mode")?.as_str()?.to_string(),
            height: info.get("height")?.as_i64()?,
            slot: info.get("slot")?.as_i64()?,
            tip: info.get("tip").and_then(Value::as_str).unwrap_or_default().to_string(),
            lib_slot: info.get("lib_slot").and_then(Value::as_i64).unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetworkInfo {
    pub peers: i64,
    pub connections: i64,
}

impl NetworkInfo {
    pub fn parse(info: &Value) -> Option<NetworkInfo> {
        Some(NetworkInfo {
            peers: info.get("n_peers")?.as_i64()?,
            connections: info.get("n_connections").and_then(Value::as_i64).unwrap_or(0),
        })
    }
}

/// The slot "now", from the node's clock.
pub fn current_slot(time_info: &Value) -> Option<i64> {
    time_info.get("current_slot")?.as_i64()
}

/// A node is at the tip when its newest block is under this many slots old.
pub const TIP_LAG_SLOTS: i64 = 180;

/// Everything the Node screen shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NodeSnapshot {
    pub chain_id: Option<String>,
    pub chain: Option<ChainInfo>,
    pub network: Option<NetworkInfo>,
    pub current_slot: Option<i64>,
    pub error: Option<String>,
}

impl NodeSnapshot {
    pub fn lag(&self) -> Option<i64> {
        Some(self.current_slot? - self.chain.as_ref()?.slot)
    }
    pub fn at_tip(&self) -> bool {
        self.lag().is_some_and(|lag| lag < TIP_LAG_SLOTS)
    }
}

/// What a `newBlock` event carries: `{"block": "<block JSON>"}`, or `null` when
/// the node's stream ended.
#[derive(Debug, Clone, PartialEq)]
pub enum BlockEvent {
    /// A header carries its slot and its parent's id; blocks have no height.
    Block { slot: Option<i64>, parent: Option<String> },
    StreamEnded,
}

pub fn block_event(block_json: &str) -> BlockEvent {
    let Ok(outer) = serde_json::from_str::<Value>(block_json) else {
        return BlockEvent::Block { slot: None, parent: None };
    };
    if outer.is_null() {
        return BlockEvent::StreamEnded;
    }
    let block = match outer.get("block") {
        Some(Value::String(text)) => serde_json::from_str(text).unwrap_or(Value::Null),
        Some(other) => other.clone(),
        None => outer.clone(),
    };
    let header = block.get("header").unwrap_or(&block);
    BlockEvent::Block {
        slot: header.get("slot").and_then(Value::as_i64),
        parent: header.get("parent_block").and_then(Value::as_str).map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_result_reply_yields_the_document_in_its_value() {
        let reply = json!({"success": true, "value": "{\"height\":7}", "error": ""});
        assert_eq!(result_value(&reply).unwrap(), json!({"height": 7}));
        let refused = json!({"success": false, "value": null, "error": "The node is not running."});
        assert_eq!(result_value(&refused).unwrap_err(), "The node is not running.");
        assert_eq!(result_value(&json!("plain")).unwrap(), json!("plain"));
    }

    #[test]
    fn chain_info_and_lag() {
        let info = json!({"lib": "00", "lib_slot": 0, "tip": "ab", "slot": 1000, "height": 42, "mode": "Online"});
        let snapshot = NodeSnapshot {
            chain: ChainInfo::parse(&info),
            current_slot: current_slot(&json!({"current_slot": 1100})),
            ..NodeSnapshot::default()
        };
        assert_eq!(snapshot.chain.as_ref().unwrap().height, 42);
        assert_eq!(snapshot.lag(), Some(100));
        assert!(snapshot.at_tip());
        assert!(ChainInfo::parse(&json!({"height": 1})).is_none());
    }

    #[test]
    fn block_events_decode_both_shapes_and_the_end_of_the_stream() {
        let block = json!({"header": {"version": "Bedrock", "parent_block": "cafe", "slot": 9}}).to_string();
        let event = json!({"block": block}).to_string();
        assert_eq!(block_event(&event), BlockEvent::Block { slot: Some(9), parent: Some("cafe".into()) });
        assert_eq!(block_event("null"), BlockEvent::StreamEnded);
        assert_eq!(block_event("{}"), BlockEvent::Block { slot: None, parent: None });
    }
}
