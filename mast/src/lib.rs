#![allow(unused)]

mod mermaid;
mod storage;
mod treap;

#[cfg(test)]
mod test {
    use super::mermaid;
    use super::treap::Treap;

    #[test]
    fn basic() {
        let mut tree = Treap::default();

        for i in 0..4 {
            tree.insert(&[i], b"0");
        }

        dbg!(&tree);
        // println!("{}", tree.as_mermaid_graph())
    }
}
