use std::{collections::HashMap, str::FromStr, sync::Arc};

use kademlia_rs::key::Key;
use tauri::AppHandle;
use tokio::sync::Mutex;

use crate::manager::Manager;

type Buckets = HashMap<u8, Vec<(String, String, String)>>;
type Record = (String, String, String);
type Records = Vec<Record>;

#[taurpc::ipc_type]
#[derive(Default)]
pub struct NodeInfo {
    pub key: String,
    pub addr: String,
    pub is_bootstrap: bool,
    pub buckets: Buckets,
    pub records: Records,
    pub last_get_res: Option<Record>,
}

#[taurpc::ipc_type]
pub struct RoutingTableChanged {
    pub node_key: String,
    // Hashmap containing (Key, Addr, Status) for each node, for a specific bucket-idx
    pub buckets: Buckets,
}

#[taurpc::ipc_type]
pub struct RecordStoreChanged {
    pub node_key: String,
    // Vec containing records: (key, publisher, value)
    pub records: Records,
}

#[taurpc::procedures(export_to = "../src/lib/bindings.ts", event_trigger = ApiEventTrigger)]
pub trait Api {
    async fn new_node(app_handle: AppHandle) -> Result<NodeInfo, ()>;

    async fn add_bootstrap_node(app_handle: AppHandle, key: String);

    async fn remove_bootstrap_node(app_handle: AppHandle, key: String);

    async fn bootstrap(node_key: String);

    async fn disconnect_peer(node_id: String, connect_peer_id: String);

    async fn close_node(app_handle: AppHandle, node_id: String) -> Result<(), ()>;

    async fn get_record(node_key: String, record_key: String);

    async fn put_record(node_key: String, record_key: Option<String>, value: String);

    async fn remove_record(node_key: String, record_key: String);

    #[taurpc(event)]
    async fn bootstrap_nodes_changed(bootstrap_nodes: Vec<(String, String)>);

    #[taurpc(event)]
    async fn routing_table_changed(routing_table: RoutingTableChanged);

    #[taurpc(event)]
    async fn record_store_changed(records: RecordStoreChanged);

    #[taurpc(event)]
    async fn get_record_finished(node_key: String, record: Record);
}

type State = Arc<Mutex<Manager>>;

#[derive(Clone)]
pub struct ApiImpl {
    pub manager: State,
}

#[taurpc::resolvers]
impl Api for ApiImpl {
    async fn new_node(self, app_handle: AppHandle) -> Result<NodeInfo, ()> {
        let mut manager = self.manager.lock().await;
        manager.init_node(app_handle).await
    }

    async fn add_bootstrap_node(self, app_handle: AppHandle, key: String) {
        let mut manager = self.manager.lock().await;
        // TODO: return invalid key error
        let key = Key::from_str(&key).unwrap();
        manager.add_bootstrap_node(key, app_handle);
    }

    async fn remove_bootstrap_node(self, app_handle: AppHandle, key: String) {
        let mut manager = self.manager.lock().await;
        // TODO: return invalid key error
        let key = Key::from_str(&key).unwrap();
        manager.remove_bootstrap_node(key, app_handle);
    }

    async fn bootstrap(self, key: String) {
        let mut manager = self.manager.lock().await;
        // TODO: return invalid key error
        let key = Key::from_str(&key).unwrap();
        manager.run_bootstrap(key);
    }

    async fn disconnect_peer(self, node_id: String, connect_peer_id: String) {
        let manager = self.manager.lock().await;
        let node_key = Key::from_str(&node_id).unwrap();
        let connect_peer_key = Key::from_str(&connect_peer_id).unwrap();
        manager.disconnect_peer(node_key, connect_peer_key);
    }

    async fn get_record(self, node_key: String, record_key: String) {
        let mut manager = self.manager.lock().await;
        let key = Key::from_str(&node_key).unwrap();
        manager.get_record(key, record_key);
    }

    async fn put_record(self, node_key: String, record_key: Option<String>, value: String) {
        let mut manager = self.manager.lock().await;
        let key = Key::from_str(&node_key).unwrap();
        manager.put_record(key, record_key, value);
    }

    async fn remove_record(self, node_key: String, record_key: String) {
        let mut manager = self.manager.lock().await;
        let key = Key::from_str(&node_key).unwrap();
        manager.remove_record(key, record_key);
    }

    async fn close_node(self, app_handle: AppHandle, node_key: String) -> Result<(), ()> {
        let key = Key::from_str(&node_key).unwrap();
        let mut manager = self.manager.lock().await;
        manager.close_node(key, app_handle)
    }
}
