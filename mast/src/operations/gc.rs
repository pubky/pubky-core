use blake3::Hash;
use redb::{Database, ReadableTable, Table, TableDefinition, WriteTransaction};

#[derive(Debug)]
enum RefCountDiff {
    Increment,
    Decrement,
}

pub(crate) fn increment_ref_count(node: Option<Hash>, table: &mut Table<&[u8], (u64, &[u8])>) {
    update_ref_count(node, RefCountDiff::Increment, table);
}

pub(crate) fn decrement_ref_count(node: Option<Hash>, table: &mut Table<&[u8], (u64, &[u8])>) {
    update_ref_count(node, RefCountDiff::Decrement, table);
}

fn update_ref_count(
    node: Option<Hash>,
    ref_diff: RefCountDiff,
    table: &mut Table<&[u8], (u64, &[u8])>,
) {
    if let Some(hash) = node {
        let mut existing = table
            .get(hash.as_bytes().as_slice())
            .unwrap()
            .expect("node shouldn't be messing!");

        let (ref_count, bytes) = {
            let (r, v) = existing.value();
            (r, v.to_vec())
        };
        drop(existing);

        let ref_count = match ref_diff {
            RefCountDiff::Increment => ref_count + 1,
            RefCountDiff::Decrement => {
                if ref_count > 0 {
                    ref_count - 1
                } else {
                    ref_count
                }
            }
        };

        match ref_count {
            0 => {
                // TODO: Confirm (read: test) this, because it is not easy to see in graphs.
                table.remove(hash.as_bytes().as_slice());
            }
            _ => {
                table.insert(hash.as_bytes().as_slice(), (ref_count, bytes.as_slice()));
            }
        }
    }
}
