mod node;
use std::io;

use node::KademliaNode;

fn main() -> io::Result<()> {
    let node = KademliaNode::new();
    println!("{:?}", node);

    Ok(())
}
