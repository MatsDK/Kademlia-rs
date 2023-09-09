// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures::StreamExt;
use kademlia_rs::{
    key::Key,
    multiaddr::{multiaddr, Multiaddr},
    node::{GetRecordResult, KademliaNode, OutEvent, PutRecordError, PutRecordOk, QueryResult},
    query::Quorum,
    routing::Node,
    store::Record,
};
use std::{collections::HashMap, str::FromStr, sync::Arc};
use tauri::AppHandle;
use tokio::{
    net::TcpListener,
    sync::broadcast::{channel, Receiver, Sender},
    sync::Mutex,
};

type Buckets = HashMap<u8, Vec<(String, String, String)>>;
type Records = Vec<(String, String, String)>;

#[taurpc::ipc_type]
#[derive(Default)]
struct NodeInfo {
    key: String,
    addr: String,
    is_bootstrap: bool,
    buckets: Buckets,
    records: Records,
}

#[taurpc::ipc_type]
struct RoutingTableChanged {
    node_key: String,
    // Hashmap containing (Key, Addr, Status) for each node, for a specific bucket-idx
    buckets: Buckets,
}

#[taurpc::ipc_type]
struct RecordStoreChanged {
    node_key: String,
    // Vec containing records: (key, publisher, value)
    records: Records,
}

#[taurpc::procedures(export_to = "../src/lib/bindings.ts", event_trigger = ApiEventTrigger)]
trait Api {
    async fn new_node(app_handle: AppHandle) -> Result<NodeInfo, ()>;

    async fn add_bootstrap_node(app_handle: AppHandle, key: String);

    async fn remove_bootstrap_node(app_handle: AppHandle, key: String);

    // async fn disconnect_node(node_id: String, connect_peer_id: String);

    // async fn close_node(node_id: String);

    async fn put_record(
        // app_handle: AppHandle,
        node_key: String,
        record_key: Option<String>,
        value: String,
    );

    #[taurpc(event)]
    async fn bootstrap_nodes_changed(bootstrap_nodes: Vec<(String, String)>);

    #[taurpc(event)]
    async fn routing_table_changed(routing_table: RoutingTableChanged);

    #[taurpc(event)]
    async fn record_store_changed(records: RecordStoreChanged);
}

#[derive(Clone)]
struct ApiImpl {
    manager: State,
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

    async fn put_record(
        self,
        // app_handle: AppHandle,
        node_key: String,
        record_key: Option<String>,
        value: String,
    ) {
        let mut manager = self.manager.lock().await;
        let key = Key::from_str(&node_key).unwrap();
        manager.put_record(key, record_key, value);
    }
}

type State = Arc<Mutex<Manager>>;

#[derive(Debug, Clone)]
enum KadEvent {
    PutRecord { record: Record },
}

#[derive(Default)]
struct Manager {
    nodes: HashMap<Key, (Multiaddr, Sender<KadEvent>)>,
    bootstrap_nodes: Vec<(Key, Multiaddr)>,
}

// Could maybe use a crate that does this
async fn get_available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_addr = listener.local_addr();
    local_addr.unwrap().port()
}

impl Manager {
    async fn init_node(&mut self, app_handle: AppHandle) -> Result<NodeInfo, ()> {
        let is_bootstrap = self.nodes.len() == 0;

        let key = Key::random();
        let addr = multiaddr!(Ip4([127, 0, 0, 1]), Tcp(get_available_port().await));

        let (tx, rx) = channel(32);

        // This function should return an error that implements thiserror::Error
        let mut node = KademliaNode::new(key, addr.clone()).await.unwrap();
        self.nodes.insert(key, (node.get_addr().clone(), tx));

        if is_bootstrap {
            self.add_bootstrap_node(key, app_handle.clone());
        } else {
            for (key, addr) in self.bootstrap_nodes.iter() {
                node.add_address(key, addr.clone());
            }
        }

        execute_node(node, is_bootstrap, rx, app_handle);

        Ok(NodeInfo {
            key: key.to_string(),
            addr: addr.to_string(),
            is_bootstrap,
            ..Default::default()
        })
    }

    fn add_bootstrap_node(&mut self, key: Key, app_handle: AppHandle) {
        if self
            .bootstrap_nodes
            .iter()
            .find(|(k, _)| *k == key)
            .is_some()
        {
            println!("already a bootstrap node");
            // Already bootstrap node
            return;
        }

        if let Some((addr, _)) = self.nodes.get(&key) {
            self.bootstrap_nodes.push((key, addr.clone()));
            self.trigger_bootstrap_node_update(app_handle);
        }
    }

    fn remove_bootstrap_node(&mut self, key: Key, app_handle: AppHandle) {
        self.bootstrap_nodes.retain(|(k, _)| *k != key);
        self.trigger_bootstrap_node_update(app_handle);
    }

    fn trigger_bootstrap_node_update(&self, app_handle: AppHandle) {
        let event_trigger = ApiEventTrigger::new(app_handle);
        event_trigger
            .bootstrap_nodes_changed(
                self.bootstrap_nodes
                    .iter()
                    .map(|(key, addr)| (key.to_string(), addr.to_string()))
                    .collect::<Vec<_>>(),
            )
            .unwrap();
    }

    fn put_record(&mut self, node_key: Key, record_key: Option<String>, value: String) {
        let node_sender = match self.nodes.get(&node_key) {
            Some((_, sender)) => sender,
            None => return,
        };

        let key = match record_key {
            // TODO: return error if invalid key
            Some(key) => Key::from_str(&key).unwrap(),
            None => Key::random(),
        };
        let record = Record {
            key,
            value: value.as_bytes().to_vec(),
            publisher: Some(node_key),
        };

        let event = KadEvent::PutRecord { record };
        node_sender.send(event).unwrap();
    }
}

fn trigger_routing_table_update(node: &KademliaNode, app_handle: AppHandle) {
    let routing_table = node.get_routing_table();

    let buckets = routing_table
        .iter()
        .map(|(idx, nodes)| {
            let nodes = nodes
                .iter()
                .map(|Node { key, addr, status }| {
                    (key.to_string(), addr.to_string(), status.to_string())
                })
                .collect::<Vec<_>>();

            (idx.clone(), nodes)
        })
        .collect();

    let event = RoutingTableChanged {
        node_key: node.local_key().to_string(),
        buckets,
    };

    let event_trigger = ApiEventTrigger::new(app_handle);
    event_trigger.routing_table_changed(event).unwrap();
}

fn trigger_store_change_update(node: &KademliaNode, app_handle: AppHandle) {
    let records = node.get_record_store();
    let records = records
        .iter()
        .map(
            |Record {
                 key,
                 value,
                 publisher,
             }| {
                let publisher = publisher.map(|v| v.to_string()).unwrap_or_default();
                let value = match String::from_utf8(value.clone()) {
                    Ok(v) => v,
                    Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
                };
                (key.to_string(), publisher, value)
            },
        )
        .collect();

    let event = RecordStoreChanged {
        node_key: node.local_key().to_string(),
        records,
    };

    let event_trigger = ApiEventTrigger::new(app_handle);
    event_trigger.record_store_changed(event).unwrap();
}

fn execute_node(
    mut node: KademliaNode,
    is_bootstrap: bool,
    mut cmd_receiver: Receiver<KadEvent>,
    app_handle: AppHandle,
) {
    tokio::spawn(async move {
        let app_handle = app_handle.clone();

        if !is_bootstrap {
            // TODO: better error handling, return error event to client
            node.bootstrap().unwrap();
        }

        loop {
            tokio::select! {
                cmd = cmd_receiver.recv() => {
                    if  let Err(e) = cmd {
                        println!("Error: {:?}", e);
                        continue
                    }
                    match cmd.unwrap() {
                        KadEvent::PutRecord { record} => {
                            node.put_record(record, Quorum::One).unwrap();
                        }
                    };
                }
                ev = node.select_next_some() => {
                    match ev {
                        OutEvent::ConnectionEstablished(_peer_id) => trigger_routing_table_update(&node, app_handle.clone()),
                        OutEvent::ConnectionClosed(_peer_id) => trigger_routing_table_update(&node, app_handle.clone()),
                        OutEvent::StoreChanged(_change) => trigger_store_change_update(&node, app_handle.clone()),
                        OutEvent::OutBoundQueryProgressed { result } => {
                            match result {
                                QueryResult::FindNode { nodes, target } => {
                                    println!("> Found nodes closest to {target}");
                                    for node in nodes {
                                        println!("\t{node}");
                                    }
                                }
                                QueryResult::PutRecord(result) => match result {
                                    Ok(PutRecordOk { key }) => {
                                        println!("> Put record {key} finished");
                                    }
                                    Err(err) => match err {
                                        PutRecordError::QuorumFailed { key, successfull_peers, quorum } => {
                                            println!("> Put record {key} quorm failed: {quorum} success: {successfull_peers:?}");
                                        }
                                    }
                                },
                                QueryResult::GetRecord(result) => match result {
                                    GetRecordResult::FoundRecord(record) => {
                                        println!("> Get record finished: {record}");
                                    }
                                    GetRecordResult::NotFound(key) => {
                                        println!("> Get record {key} failed: NotFound")
                                    }
                                },
                                QueryResult::Bootstrap => {
                                    println!("> Successfull bootstrap")
                                }
                            }
                        }
                        OutEvent::Other => {}
                    }
                }
            }
        }
    });
}

#[tokio::main]
async fn main() {
    let manager = Arc::new(Mutex::new(Manager::default()));
    tauri::Builder::default()
        .invoke_handler(taurpc::create_ipc_handler(
            ApiImpl { manager }.into_handler(),
        ))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
