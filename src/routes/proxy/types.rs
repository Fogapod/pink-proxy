use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct ProxyURL {
    pub url: String,
    pub ttl: u64,
}

#[derive(Serialize)]
pub struct ProxyID {
    pub id: Uuid,
}

#[derive(Debug, Clone)]
pub struct Proxy {
    pub url: String,
    pub valid_until: Instant,
}

#[derive(Debug)]
pub struct Proxies(pub Mutex<HashMap<Uuid, Proxy>>);
