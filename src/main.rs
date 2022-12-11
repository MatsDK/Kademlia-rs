use std::io;

mod key;
mod node;

use key::Key;
use node::KademliaNode;

pub const K_VALUE: usize = 4;

fn main() -> io::Result<()> {
    let key = Key::random();
    // println!("{}", key.distance(&key2).diff_bits().unwrap());

    let mut node = KademliaNode::new(key);
    let mut nodes = Vec::new();

    for _ in 0..20 {
        let key = Key::random();

        let mut new_node = KademliaNode::new(key);
        new_node.add_address(node.local_key());
        node.add_address(new_node.local_key());
        nodes.push(new_node);
    }

    node.find_nodes(nodes[5].local_key());

    Ok(())
}
