#![allow(unused)]

mod node;
mod storage;
mod tree;

use crate::node::Node;

// // TODO: maybe add nonce for encrypted  trees.
// // TODO: why add header in each node? or in the Mast commit?
// //  Would we need to read a node without traversing down from the Mast commit?
// pub struct Mast {
//     /// The name of this Mast to be used as a prefix for all nodes
//     /// in the storage, seperating different Masts.
//     name: String,
//     root: Option<Hash>,
//     storage: MastStorage,
// }
