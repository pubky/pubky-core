use blake3::{Hash, Hasher};
use redb::{Database, ReadableTable, Table, TableDefinition};

use crate::node::{Branch, Node};

// TODO: remove unused
// TODO: remove unwrap

#[derive(Debug)]
pub struct HashTreap<'a> {
    /// Redb database to store the nodes.
    pub(crate) db: &'a Database,
    pub(crate) root: Option<Node>,
}

// Table: Nodes v0
// Key:   `[u8; 32]`    # Node hash
// Value: `(u64, [u8])` # (RefCount, EncodedNode)
const NODES_TABLE: TableDefinition<&[u8], (u64, &[u8])> =
    TableDefinition::new("kytz:hash_treap:nodes:v0");

impl<'a> HashTreap<'a> {
    // TODO: add name to open from storage with.
    pub fn new(db: &'a Database) -> Self {
        // Setup tables

        let write_tx = db.begin_write().unwrap();
        {
            let _table = write_tx.open_table(NODES_TABLE).unwrap();
        }
        write_tx.commit().unwrap();

        // TODO: Try to open root (using this treaps or tags table).
        // TODO: sould be checking for root on the fly probably!

        Self { root: None, db }
    }

    pub fn insert(&mut self, key: &[u8], value: &[u8]) {
        // TODO: validate key and value length.

        let mut node = Node::new(key, value);

        let write_txn = self.db.begin_write().unwrap();

        let _ = 'transaction: {
            let mut nodes_table = write_txn.open_table(NODES_TABLE).unwrap();

            if self.root.is_none() {
                // We are done.
                self.update_root(&node, &mut nodes_table);

                break 'transaction;
            }

            // Watch this [video](https://youtu.be/NxRXhBur6Xs?si=GNwaUOfuGwr_tBKI&t=1763) for a good explanation of the unzipping algorithm.
            // Also see the Iterative insertion algorithm in the page 12 of the [original paper](https://arxiv.org/pdf/1806.06726.pdf).
            // The difference here is that in a Hash Treap, we need to update nodes bottom up.

            // Let's say we have the following tree:
            //
            //         F
            //        / \
            //       D   P
            //      /   / \
            //     C   H   X
            //    /   / \   \
            //   A   G   M   Y
            //          /
            //         I
            //
            // First we mark the binary search path to the leaf, going right if the key is greater than
            // the current node's key and vice versa.
            //
            //         F
            //          \
            //           P
            //          /
            //         H
            //          \
            //           M
            //          /
            //         I
            //

            // Path before insertion point. (Node, Branch to update)
            let mut top_path: Vec<(Node, Branch)> = Vec::new();
            // Subtree of nodes on the path smaller than the inserted key.
            let mut left_unzip_path: Vec<Node> = Vec::new();
            // Subtree of nodes on the path larger  than the inserted key.
            let mut right_unzip_path: Vec<Node> = Vec::new();

            let mut next = self.root.clone().map(|n| n.hash());

            // Top down traversal of the binary search path.
            while let Some(current) = self.get_node(&next) {
                let should_zip = node.rank().as_bytes() > current.rank().as_bytes();

                // Traverse left or right.
                if key < current.key() {
                    next = *current.left();

                    if should_zip {
                        right_unzip_path.push(current)
                    } else {
                        top_path.push((current, Branch::Left));
                    }
                } else {
                    next = *current.right();

                    if should_zip {
                        left_unzip_path.push(current)
                    } else {
                        top_path.push((current, Branch::Right));
                    }
                };
            }

            // === Updating hashes bottom up ===

            // We are at the unzipping part of the path.
            //
            // First do the unzipping bottom up.
            //
            //         H
            //          \
            //           M    < current_right
            //          /
            //         I      < current_left
            //
            // Into (hopefully you can see the "unzipping"):
            //
            //  left     right
            //  subtree  subtree
            //
            //    H    |
            //      \  |
            //       I |  M

            while left_unzip_path.len() > 1 {
                let child = left_unzip_path.pop().unwrap();
                let mut parent = left_unzip_path.last_mut().unwrap();

                parent.set_child(&Branch::Right, Some(child.hash()), &mut nodes_table);
            }

            while right_unzip_path.len() > 1 {
                let child = right_unzip_path.pop().unwrap();
                let mut parent = right_unzip_path.last_mut().unwrap();

                parent.set_child(&Branch::Left, Some(child.hash()), &mut nodes_table);
            }

            // Done unzipping, join the current_left and current_right to J and update hashes upwards.
            //
            //         J     < Insertion point.
            //        / \
            //       H   M
            //        \
            //         I

            node.set_child(
                &Branch::Left,
                left_unzip_path.first().map(|n| n.hash()),
                &mut nodes_table,
            );
            node.set_child(
                &Branch::Right,
                right_unzip_path.first().map(|n| n.hash()),
                &mut nodes_table,
            );

            // Update the rest of the path upwards with the new hashes.
            // So the final tree should look like:
            //
            //         F
            //        / \
            //       D   P
            //      /   / \
            //     C   J   X
            //    /   / \   \
            //   A   H   M   Y
            //      / \
            //     G   I

            if top_path.is_empty() {
                // The insertion point is at the root and we are done.
                self.update_root(&node, &mut nodes_table)
            }

            let mut previous = node;

            while let Some((mut parent, branch)) = top_path.pop() {
                parent.set_child(&branch, Some(previous.hash()), &mut nodes_table);

                previous = parent;
            }

            // Update the root pointer.
            self.update_root(&previous, &mut nodes_table)
        };

        // Finally we should commit the changes to the storage.
        write_txn.commit().unwrap();
    }

    // === Private Methods ===

    fn update_root(&mut self, node: &Node, table: &mut Table<&[u8], (u64, &[u8])>) {
        node.save(table);

        // The tree is empty, the incoming node has to be the root, and we are done.
        self.root = Some(node.clone());

        // TODO: we need to persist the root change too to the storage.
    }

    pub(crate) fn get_node(&self, hash: &Option<Hash>) -> Option<Node> {
        let read_txn = self.db.begin_read().unwrap();
        let table = read_txn.open_table(NODES_TABLE).unwrap();

        hash.and_then(|h| {
            table
                .get(h.as_bytes().as_slice())
                .unwrap()
                .map(|existing| Node::decode(existing.value()))
        })
    }

    // === Test Methods ===

    #[cfg(test)]
    fn verify_ranks(&self) -> bool {
        let node = self.get_node(&self.root.clone().map(|n| n.hash()));
        self.check_rank(node)
    }

    #[cfg(test)]
    fn check_rank(&self, node: Option<Node>) -> bool {
        match node {
            Some(n) => {
                let left_check = self.get_node(n.left()).map_or(true, |left| {
                    n.rank().as_bytes() > left.rank().as_bytes() && self.check_rank(Some(left))
                });
                let right_check = self.get_node(n.right()).map_or(true, |right| {
                    n.rank().as_bytes() > right.rank().as_bytes() && self.check_rank(Some(right))
                });

                left_check && right_check
            }
            None => true,
        }
    }
}

#[cfg(test)]
mod test {
    use super::HashTreap;
    use super::Node;

    use redb::{Database, Error, ReadableTable, TableDefinition};

    #[test]
    fn basic() {
        // Create an in-memory database
        let file = tempfile::NamedTempFile::new().unwrap();
        let db = Database::create(file.path()).unwrap();

        let mut treap = HashTreap::new(&db);

        let mut keys = [
            "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
            "R", "S", "T", "U", "V", "W", "X", "Y", "Z", //
            "A0", "B0", "C0", "D0", "E0", "F0", "G0", "H0", "I0", "J0", "K0", "L0", "M0", "N0",
            "O0", "P0", "Q0", "R0", "S0", "T0", "U0", "V0", "W0", "X0", "Y0", "Z0", //
            "A1", "B1", "C1", "D1", "E1", "F1", "G1", "H1", "I1", "J1", "K1", "L1", "M1", "N1",
            "O1", "P1", "Q1", "R1", "S1", "T1", "U1", "V1", "W1", "X1", "Y1", "Z1",
        ];
        let mut keys = [
            "abacus",
            "abdomen",
            "abdominal",
            "abide",
            "abiding",
            "ability",
            "ablaze",
            "able",
            "abnormal",
            "abrasion",
            "abrasive",
            "abreast",
            "abridge",
            "abroad",
            "abruptly",
            "absence",
            "absentee",
            "absently",
            "absinthe",
            "absolute",
            "absolve",
            "abstain",
            "abstract",
            "absurd",
            "accent",
            "acclaim",
            "acclimate",
            "accompany",
            "account",
            "accuracy",
            "accurate",
            "accustom",
            "acetone",
            "achiness",
            "aching",
            "acid",
            "acorn",
            "acquaint",
            "acquire",
            "acre",
            "acrobat",
            "acronym",
            "acting",
            "action",
            "activate",
            "activator",
            "active",
            "activism",
            "activist",
            "activity",
            "actress",
            "acts",
            "acutely",
            "acuteness",
            "aeration",
            "aerobics",
            "aerosol",
            "aerospace",
            "afar",
            "affair",
            "affected",
            "affecting",
            "affection",
            "affidavit",
            "affiliate",
            "affirm",
            "affix",
            "afflicted",
            "affluent",
            "afford",
            "affront",
            "aflame",
            "afloat",
            "aflutter",
            "afoot",
            "afraid",
            "afterglow",
            "afterlife",
            "aftermath",
            "aftermost",
            "afternoon",
            "aged",
            "ageless",
            "agency",
            "agenda",
            "agent",
            "aggregate",
            "aghast",
            "agile",
            "agility",
            "aging",
            "agnostic",
            "agonize",
            "agonizing",
            "agony",
            "agreeable",
            "agreeably",
            "agreed",
            "agreeing",
            "agreement",
            "aground",
            "ahead",
            "ahoy",
            "aide",
            "aids",
            "aim",
            "ajar",
            "alabaster",
            "alarm",
            "albatross",
            "album",
            "alfalfa",
            "algebra",
            "algorithm",
            "alias",
            "alibi",
            "alienable",
            "alienate",
            "aliens",
            "alike",
        ];

        for key in keys.iter() {
            treap.insert(key.as_bytes(), b"0");
        }

        assert!(treap.verify_ranks());
        println!("{}", treap.as_mermaid_graph())
    }

    fn failin_cases() {
        // Create an in-memory database
        let file = tempfile::NamedTempFile::new().unwrap();
        let db = Database::create(file.path()).unwrap();

        let mut treap = HashTreap::new(&db);

        // TODO: fix this cases
        let mut keys = [
            "D", "N", "P", "X", "F", "Z", "Y", "A", "G", "C", "M", "H", "I", "J",
        ];

        for key in keys.iter() {
            treap.insert(key.as_bytes(), b"0");
        }

        assert!(treap.verify_ranks());
        println!("{}", treap.as_mermaid_graph())
    }
}
