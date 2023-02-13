use clap::Parser;
use futures::StreamExt;
use multiaddr::Multiaddr;
use std::io;
use tokio::io::{stdin, AsyncBufReadExt, BufReader};

mod key;
mod node;
mod pool;
mod query;
mod routing;
mod transport;

use crate::key::Key;
use crate::node::{KademliaNode, OutEvent};

pub const K_VALUE: usize = 4;

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
    let key = Key::random();
    let Args { addr, dial } = Args::parse();
    let mut node = KademliaNode::new(key, addr).await?;

    if let Some(dial) = dial {
        // node.add_address(&key, addr);
        // node.boostrap(dial).await?;
    }

    let mut reader = BufReader::new(stdin()).lines();

    loop {
        tokio::select! {
            Ok(Some(_line)) = reader.next_line() => {
                let key = Key::random();
                node.find_node(&key);
            }
            ev = node.select_next_some() => {
                match ev {
                    OutEvent::ConnectionEstablished( peer_id) => {
                        println!("> Connection established: {}", peer_id);
                    }
                    OutEvent::OutBoundQueryProgressed {result} => {
                        println!("> Query progressed: {:?}", result);
                    }
                    OutEvent::Other => {}
                }
            }
        }
    }
}
