#[cfg(test)]
mod test {
    use crate::{Node, Treap};

    impl<'a> Treap<'a> {
        pub fn as_mermaid_graph(&self) -> String {
            let mut graph = String::new();

            graph.push_str("graph TD;\n");

            if let Some(root) = &self.root {
                self.build_graph_string(&root, &mut graph);
            }

            graph
        }

        fn build_graph_string(&self, node: &Node, graph: &mut String) {
            let key = bytes_to_string(node.key());
            let node_label = format!("{}({})", key, key);

            graph.push_str(&format!("    {};\n", node_label));

            if let Some(child) = self.get_node(node.left()) {
                let key = bytes_to_string(child.key());
                let child_label = format!("{}({})", key, key);

                graph.push_str(&format!("    {} --> {};\n", node_label, child_label));
                self.build_graph_string(&child, graph);
            }

            if let Some(child) = self.get_node(node.right()) {
                let key = bytes_to_string(child.key());
                let child_label = format!("{}({})", key, key);

                graph.push_str(&format!("    {} --> {};\n", node_label, child_label));
                self.build_graph_string(&child, graph);
            }
        }
    }

    fn bytes_to_string(byte: &[u8]) -> String {
        String::from_utf8(byte.to_vec()).expect("Invalid utf8 key in test with mermaig graph")
    }
}
