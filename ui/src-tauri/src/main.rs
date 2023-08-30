// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use futures::StreamExt;
use kademlia_rs::{
    key::Key,
    multiaddr::{multiaddr, Multiaddr},
    node::{GetRecordResult, KademliaNode, OutEvent, PutRecordError, PutRecordOk, QueryResult},
};
use std::{collections::HashMap, ops::Index, str::FromStr, sync::Arc};
use tauri::{utils::config::CliArg, AppHandle};
use tokio::{
    net::TcpListener,
    sync::broadcast::{channel, Sender},
    sync::Mutex,
};

#[taurpc::ipc_type]
struct NodeInfo {
    key: String,
    addr: String,
    is_bootstrap: bool,
}

#[taurpc::procedures(export_to = "../src/lib/bindings.ts", event_trigger = ApiEventTrigger)]
trait Api {
    async fn new_node(app_handle: AppHandle) -> Result<NodeInfo, ()>;

    async fn add_bootstrap_node(app_handle: AppHandle, key: String);

    async fn remove_bootstrap_node(app_handle: AppHandle, key: String);

    #[taurpc(event)]
    async fn bootstrap_nodes_changed(bootstrap_nodes: Vec<(String, String)>);
}

#[derive(Clone)]
struct ApiImpl {
    manager: State,
}

#[taurpc::resolvers]
impl Api for ApiImpl {
    async fn new_node(self, app_handle: AppHandle) -> Result<NodeInfo, ()> {
        let mut state = self.manager.lock().await;
        state.init_node(app_handle).await
    }

    async fn add_bootstrap_node(self, app_handle: AppHandle, key: String) {
        let mut state = self.manager.lock().await;
        // TODO: return invalid key error
        let key = Key::from_str(&key).unwrap();
        state.add_bootstrap_node(key, app_handle)
    }

    async fn remove_bootstrap_node(self, app_handle: AppHandle, key: String) {
        let mut state = self.manager.lock().await;
        // TODO: return invalid key error
        let key = Key::from_str(&key).unwrap();
        state.remove_bootstrap_node(key, app_handle)
    }
}

type State = Arc<Mutex<Manager>>;

#[derive(Default)]
struct Manager {
    nodes: HashMap<Key, (Multiaddr, Sender<()>)>,
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
            self.add_bootstrap_node(key, app_handle);
        } else {
            for (key, addr) in self.bootstrap_nodes.iter() {
                node.add_address(key, addr.clone());
            }
        }

        execute_node(node, is_bootstrap);

        Ok(NodeInfo {
            key: key.to_string(),
            addr: addr.to_string(),
            is_bootstrap,
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
}

fn execute_node(mut node: KademliaNode, is_bootstrap: bool) {
    tokio::spawn(async move {
        if !is_bootstrap {
            // TODO: better error handling, return error event to client
            node.bootstrap().unwrap();
        }

        loop {
            tokio::select! {
                ev = node.select_next_some() => {
                    match ev {
                        OutEvent::ConnectionEstablished( peer_id) => {
                            println!("> Connection established: {}", peer_id);
                        }
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
