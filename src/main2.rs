use multiaddr::Multiaddr;
use std::io;

mod key;
mod node;
mod transport;

use key::Key;
use node::KademliaNode;

pub const K_VALUE: usize = 4;

#[tokio::main]
async fn main() -> io::Result<()> {
    let addr = "/ip4/127.0.0.1/tcp/10501".parse::<Multiaddr>().unwrap();
    let key = Key::random();

    let _node = KademliaNode::new(key, addr).await?;

    // node.dial("127.0.0.1:10501".parse::<Multiaddr>().unwrap()).await?;

    // let nodes = node.find_nodes(&key2);
    // println!("{nodes:?}");

    loop {}
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

    Ok(())
}
