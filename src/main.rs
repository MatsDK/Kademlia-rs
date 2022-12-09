use std::io;

mod key;
mod node;

use key::Key;
use node::KademliaNode;

fn main() -> io::Result<()> {
    let key = Key::random();
    let key2 = Key::random();

    let node = KademliaNode::new(key);
    let mut node2 = KademliaNode::new(key2);

    node2.add_address(node);

    Ok(())
}
