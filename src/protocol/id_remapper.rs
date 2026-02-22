use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::jsonrpc::JsonRpcId;

pub struct IdRemapper {
    counter: AtomicU64,
    mappings: Mutex<HashMap<u64, (JsonRpcId, String)>>,
}

impl IdRemapper {
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(1),
            mappings: Mutex::new(HashMap::new()),
        }
    }

    pub fn remap(&self, original_id: JsonRpcId, backend: &str) -> u64 {
        let gateway_id = self.counter.fetch_add(1, Ordering::Relaxed);
        let mut map = self.mappings.lock().unwrap();
        map.insert(gateway_id, (original_id, backend.to_string()));
        gateway_id
    }

    pub fn restore(&self, gateway_id: u64) -> Option<(JsonRpcId, String)> {
        let mut map = self.mappings.lock().unwrap();
        map.remove(&gateway_id)
    }

    pub fn pending_count(&self) -> usize {
        let map = self.mappings.lock().unwrap();
        map.len()
    }
}

impl Default for IdRemapper {
    fn default() -> Self {
        Self::new()
    }
}
