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

use key::Key;
use node::KademliaNode;

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
    // let addr = "/ip4/127.0.0.1/tcp/10500".parse::<Multiaddr>().unwrap();
    // let addr2 = "/ip4/127.0.0.1/tcp/10501".parse::<Multiaddr>().unwrap();

    let mut node = KademliaNode::new(key, addr).await?;

    if let Some(dial) = dial {
        node.dial(dial).await?;
    }

    let mut reader = BufReader::new(stdin()).lines();

    loop {
        tokio::select! {
            Ok(Some(_line)) = reader.next_line() => {
                // println!("line: {:?}", line);
                let key = Key::random();
                node.find_node(&key);
            }
            _ev = node.select_next_some() => {

            }
        }
        // println!("{nodes:?}");
    }
    // let mut nodes = Vec::new();
    // for _ in 0..20 {
    //     let addr = multiaddr!(Ip4([127, 0, 0, 1]), Tcp(10500u16));
    //     let key = Key::random();
    //     let mut new_node = KademliaNode::new(key, addr)?;
    //     new_node.add_address(node.local_key());
    //     node.add_address(new_node.local_key());
    //     nodes.push(new_node);
    // }

    // let keys = node.find_nodes(nodes[5].local_key());

    // for k in keys.iter() {
    //     println!("{:?} {k:?}", node.local_key().distance(&k));
    // }

    // Ok(())
}
