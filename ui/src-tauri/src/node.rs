use futures::StreamExt;
use kademlia_rs::{
    key::Key,
    node::{
        FoundRecord, GetRecordResult, KademliaNode, OutEvent, PutRecordError, PutRecordOk,
        QueryResult,
    },
    query::Quorum,
    routing::Node,
    store::Record,
};
use std::num::NonZeroUsize;
use tauri::AppHandle;
use tokio::sync::broadcast::Receiver;

use crate::{
    ipc::{ApiEventTrigger, RecordStoreChanged, RoutingTableChanged},
    manager::KadEvent,
};

pub fn trigger_store_change_update(node: &KademliaNode, app_handle: AppHandle) {
    let records = node.get_record_store();
    let records = records
        .into_iter()
        .map(|r| {
            let Record {
                publisher,
                value,
                key,
                ..
            } = r.into_owned();
            let publisher = publisher.map(|v| v.to_string()).unwrap_or_default();
            let value = match String::from_utf8(value.clone()) {
                Ok(v) => v,
                Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
            };
            (key.to_string(), publisher, value)
        })
        .collect();

    let event = RecordStoreChanged {
        node_key: node.local_key().to_string(),
        records,
    };

    let event_trigger = ApiEventTrigger::new(app_handle);
    event_trigger.record_store_changed(event).unwrap();
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

pub fn get_record_finished(key: &Key, record: Record, app_handle: AppHandle) {
    let event_trigger = ApiEventTrigger::new(app_handle);
    event_trigger
        .get_record_finished(
            key.to_string(),
            (
                record.key.to_string(),
                record.publisher.map_or(String::from(""), |k| k.to_string()),
                String::from_utf8(record.value).unwrap(),
            ),
        )
        .unwrap();
}

pub fn execute_node(
    mut node: KademliaNode,
    is_bootstrap: bool,
    mut cmd_receiver: Receiver<KadEvent>,
    app_handle: AppHandle,
) {
    tokio::spawn(async move {
        let app_handle = app_handle.clone();

        if !is_bootstrap {
            // TODO: better error handling, return error event to client
            let _ = node.bootstrap();
        }

        loop {
            tokio::select! {
                cmd = cmd_receiver.recv() => {
                    if  let Err(e) = cmd {
                        println!("Error: {:?}", e);
                        continue
                    }
                    match cmd.unwrap() {
                        KadEvent::GetRecord { key } => {
                            node.get_record(key);
                        }
                        KadEvent::PutRecord { record } => {
                            node.put_record(record, Quorum::N(NonZeroUsize::new(2).unwrap())).unwrap();
                        }
                        KadEvent::RemoveRecord { key } => {
                            node.remove_record(&key);
                        }
                        KadEvent::DisconnectPeer { key } => {
                            let _ = node.disconnect(key);
                        }
                        KadEvent::Bootstrap { nodes } => {
                            for (key, addr) in nodes {
                                node.add_address(&key, addr.clone());
                            }
                            let _  = node.bootstrap();
                        }
                        KadEvent::CloseNode => {
                            drop(node);
                            return
                        }
                    };
                }
                ev = node.select_next_some() => {
                    match ev {
                        OutEvent::ConnectionEstablished(_peer_id) => trigger_routing_table_update(&node, app_handle.clone()),
                        OutEvent::ConnectionClosed { .. } => trigger_routing_table_update(&node, app_handle.clone()),
                        OutEvent::ConnectionFailed { .. } => trigger_routing_table_update(&node, app_handle.clone()),
                        OutEvent::StoreChanged(_change) => trigger_store_change_update(&node, app_handle.clone()),
                        OutEvent::OutBoundQueryProgressed { result, .. } => {
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
                                    GetRecordResult::FoundRecord(FoundRecord { record, .. }) => {
                                        println!("> Get record finished: {record}");
                                        get_record_finished(node.local_key(), record, app_handle.clone());
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
