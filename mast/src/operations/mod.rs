pub mod insert;
pub mod read;
pub mod remove;
mod search;

pub(crate) use insert::insert;
pub(crate) use remove::remove;

use redb::{ReadableTable, TableDefinition};

// Table: Nodes v0
// stores all the hash treap nodes from all the treaps in the storage.
//
// Key:   `[u8; 32]`    # Node hash
// Value: `(u64, [u8])` # (RefCount, EncodedNode)
pub const NODES_TABLE: TableDefinition<&[u8], (u64, &[u8])> =
    TableDefinition::new("kytz:hash_treap:nodes:v0");

// Table: Roots v0
// stores all the current roots for all treaps in the storage.
//
// Key:   `[u8; 32]`    # Treap name
// Value: `[u8; 32]` # Hash
pub const ROOTS_TABLE: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("kytz:hash_treap:roots:v0");
