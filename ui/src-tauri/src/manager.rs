use kademlia_rs::{
    key::Key,
    multiaddr::{multiaddr, Multiaddr},
    node::{KademliaConfig, KademliaNode},
    store::Record,
};
use std::{collections::HashMap, str::FromStr, time::Duration};
use tauri::AppHandle;
use tokio::{
    net::TcpListener,
    sync::broadcast::{channel, Sender},
};

use crate::{
    ipc::{ApiEventTrigger, NodeInfo},
    node::execute_node,
};

#[derive(Debug, Clone)]
pub enum KadEvent {
    GetRecord { key: Key },
    PutRecord { record: Record },
    RemoveRecord { key: Key },
    DisconnectPeer { key: Key },
    Bootstrap { nodes: Vec<(Key, Multiaddr)> },
    CloseNode,
}

#[derive(Default)]
pub struct Manager {
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
    pub async fn init_node(
        &mut self,
        app_handle: AppHandle,
        key: Option<String>,
        make_bootstrap: bool,
    ) -> Result<NodeInfo, ()> {
        let key = key.map_or(Key::random(), |s| Key::from_str(&s).unwrap());
        let addr = multiaddr!(Ip4([127, 0, 0, 1]), Tcp(get_available_port().await));

        let (tx, rx) = channel(32);

        let mut config = KademliaConfig::default();
        config.set_replication_interval(Some(Duration::from_secs(60)));
        config.set_publication_interval(Some(Duration::from_secs(120)));
        // This function should return an error that implements thiserror::Error
        let mut node = KademliaNode::new(key, addr.clone(), config).await.unwrap();
        self.nodes.insert(key, (node.get_addr().clone(), tx));

        if make_bootstrap {
            self.add_bootstrap_node(key, app_handle.clone());
        } else {
            for (key, addr) in self.bootstrap_nodes.iter() {
                node.add_address(key, addr.clone());
            }
        }

        execute_node(node, make_bootstrap, rx, app_handle);

        Ok(NodeInfo {
            key: key.to_string(),
            addr: addr.to_string(),
            is_bootstrap: make_bootstrap,
            ..Default::default()
        })
    }

    pub fn add_bootstrap_node(&mut self, key: Key, app_handle: AppHandle) {
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

    pub fn remove_bootstrap_node(&mut self, key: Key, app_handle: AppHandle) {
        self.bootstrap_nodes.retain(|(k, _)| *k != key);
        self.trigger_bootstrap_node_update(app_handle);
    }

    pub fn run_bootstrap(&mut self, key: Key) {
        let Some((_, node_sender)) = self.nodes.get(&key) else {
            return
        };

        let nodes = self.bootstrap_nodes.clone();
        node_sender.send(KadEvent::Bootstrap { nodes }).unwrap();
    }

    pub fn trigger_bootstrap_node_update(&self, app_handle: AppHandle) {
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

    pub fn disconnect_peer(&self, node_key: Key, connect_peer: Key) {
        let Some((_, node_sender)) = self.nodes.get(&node_key) else {
            return
        };

        node_sender
            .send(KadEvent::DisconnectPeer { key: connect_peer })
            .unwrap();
    }

    pub fn get_record(&mut self, node_key: Key, record_key: String) {
        let key = Key::from_str(&record_key).unwrap();
        let Some((_, node_sender)) = self.nodes.get(&node_key) else {
            return
        };

        node_sender.send(KadEvent::GetRecord { key }).unwrap();
    }

    pub fn put_record(&mut self, node_key: Key, record_key: Option<String>, value: String) {
        let Some((_, node_sender)) = self.nodes.get(&node_key) else {
            return
        };

        let key = match record_key {
            // TODO: return error if invalid key
            Some(key) => Key::from_str(&key).unwrap(),
            None => Key::random(),
        };

        let record = Record::new(key, value.as_bytes().to_vec());

        let event = KadEvent::PutRecord { record };
        node_sender.send(event).unwrap();
    }

    pub fn remove_record(&mut self, node_key: Key, record_key: String) {
        let key = Key::from_str(&record_key).unwrap();
        let Some((_, node_sender)) = self.nodes.get(&node_key) else {
            return
        };

        node_sender.send(KadEvent::RemoveRecord { key }).unwrap();
    }

    pub fn close_node(&mut self, node_key: Key, app_handle: AppHandle) -> Result<(), ()> {
        if let Some((_addr, sender)) = self.nodes.remove(&node_key) {
            sender.send(KadEvent::CloseNode).unwrap();
            self.remove_bootstrap_node(node_key, app_handle);
            return Ok(());
        }
        Err(())
    }
}
