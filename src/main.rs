use clap::Parser;
use futures::StreamExt;
use multiaddr::Multiaddr;
use std::io;
use std::str::FromStr;
use tokio::io::{stdin, AsyncBufReadExt, BufReader};

mod key;
mod node;
mod pool;
mod query;
mod routing;
mod store;
mod transport;

use crate::key::Key;
use crate::node::{KademliaNode, OutEvent, QueryResult};
use crate::store::Record;

pub const K_VALUE: usize = 4;
static BOOTSTRAP_NODE_KEY: &str = "5zrr7BPc5gnMV6EbdpPfxpoJfZuddRH8PK1EQQmEAPFw";

#[derive(Parser, Debug)]
struct Args {
    /// Local listening addr
    #[arg(short, long)]
    addr: Multiaddr,

    /// Dial an other peer with addr, None for bootstrapping peer
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

    let mut node = KademliaNode::new(key, addr).await?;

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

                        let record = Record {
                            key,
                            value,
                            publisher: None,
                            expires: None

                        };

                        node.put_record(record).unwrap();
                    }
                    Some("GET") => {
                        let key = {
                            match args.next() {
                                Some(key) => Key::from_str(&key).unwrap(),
                                None => Key::random()
                            }
                        };

                        println!("GET result: {:?}", node.get_record(key));
                    }
                    _ => {}
                }
            }
            ev = node.select_next_some() => {
                match ev {
                    OutEvent::ConnectionEstablished( peer_id) => {
                        println!("> Connection established: {}", peer_id);
                    }
                    OutEvent::OutBoundQueryProgressed {result} => {
                        match result {
                            QueryResult::FindNode { nodes, target } => {
                                println!("> Found nodes closest to {target}");
                                for node in nodes {
                                    println!("\t{node}");
                                }
                            }
                            QueryResult::PutRecord{.. } => {
                                println!("> Put record finished");
                            }
                        }
                    }
                    OutEvent::Other => {}
                }
            }
        }
    }
}
