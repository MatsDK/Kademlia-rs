extern crate kademlia_rs;

use clap::Parser;
use futures::StreamExt;
use multiaddr::Multiaddr;
use std::io;
use std::str::FromStr;
use tokio::io::{stdin, AsyncBufReadExt, BufReader};

use kademlia_rs::key::Key;
use kademlia_rs::node::{
    FoundRecord, GetRecordResult, KademliaConfig, KademliaNode, OutEvent, PutRecordError,
    PutRecordOk, QueryResult, StoreChangedEvent,
};
use kademlia_rs::query::Quorum;
use kademlia_rs::store::Record;

static BOOTSTRAP_NODE_KEY: &str = "5zrr7BPc5gnMV6EbdpPfxpoJfZuddRH8PK1EQQmEAPFw";

#[derive(Parser)]
struct Args {
    /// Local listening addr
    #[arg(short, long)]
    addr: Multiaddr,

    /// Dial another peer by addr, None for bootstrapping peer
    #[arg(short, long)]
    dial: Option<Multiaddr>,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let Args { addr, dial } = Args::parse();

    let key = if dial.is_some() {
        Key::random()
    } else {
        Key::from_str(BOOTSTRAP_NODE_KEY).unwrap()
    };

    let mut config = KademliaConfig::default();
    config.set_record_ttl(None);
    let mut node = KademliaNode::new(key, addr, config).await?;

    if let Some(dial) = dial {
        let key = Key::from_str(BOOTSTRAP_NODE_KEY).unwrap();
        println!(">> dialing {key} on {dial:?}");
        node.add_address(&key, dial);
        node.bootstrap().unwrap();
    }

    let mut reader = BufReader::new(stdin()).lines();

    loop {
        tokio::select! {
            Ok(Some(line)) = reader.next_line() => {
                let mut args = line.split(' ');

                match args.next() {
                    Some("FIND_NODE") => {
                        let key = {
                            match args.next() {
                                Some(key) => Key::from_str(&key).unwrap(),
                                None => Key::random()
                            }
                        };

                        node.find_node(&key);
                    }
                    Some("PUT") => {
                        let value = {
                            match args.next() {
                                Some(v) => v.as_bytes().to_vec(),
                                None => vec![]
                            }
                        };

                        let key = {
                            match args.next() {
                                Some(key) => Key::from_str(&key).unwrap(),
                                None => Key::random()
                            }
                        };

                        let record = Record::new(key, value);
                        // let q = Quorum::N(NonZeroUsize::new(4).unwrap());
                        let q = Quorum::One;
                        node.put_record(record, q).unwrap();
                    }
                    Some("GET") => {
                        let key = {
                            match args.next() {
                                Some(key) => Key::from_str(&key).unwrap(),
                                None => Key::random()
                            }
                        };

                        node.get_record(key);
                    }
                    Some("REMOVE") => {
                        let key = {
                            match args.next() {
                                Some(key) => Key::from_str(&key).unwrap(),
                                None => Key::random()
                            }
                        };

                        node.remove_record(&key);
                    }
                    _ => {}
                }
            }
            ev = node.select_next_some() => {
                match ev {
                    OutEvent::ConnectionEstablished(peer_id) => {
                        println!("> Connection established: {}", peer_id);
                    }
                    OutEvent::ConnectionClosed { peer, reason, .. } => {
                        println!("> Connection closed: {}, reason: {:?}", peer, reason);
                    }
                    #[cfg(feature = "debug")]
                    OutEvent::StoreChanged(change) => match change {
                        StoreChangedEvent::PutRecord { record } => {
                            println!("> Successfully stored record locally: {record:?}");

                        }
                        StoreChangedEvent::RemoveRecord { record_key } => {
                            println!("> Successfully remove record locally: {record_key}");
                        }
                    }
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
                                GetRecordResult::FoundRecord(FoundRecord{record, ..}) => {
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
}
