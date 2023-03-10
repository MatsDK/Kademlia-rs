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

pub const K_VALUE: usize = 4;
static BOOTSTRAP_NODE_KEY: &str = "JA73AGTSG3bhqKuEYc2LdsyQLJafQoGrDvzh5q433qDi";

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
        println!("dialing {key} on {dial:?}");
        node.add_address(&key, dial);
        node.bootstrap().unwrap();
    }

    let mut reader = BufReader::new(stdin()).lines();

    loop {
        tokio::select! {
            Ok(Some(line)) = reader.next_line() => {
                match line.as_str() {
                    "FIND_NODE" => {
                        let key = Key::random();
                        node.find_node(&key);
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
                        }
                    }
                    OutEvent::Other => {}
                }
            }
        }
    }
}
