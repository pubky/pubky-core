#[cfg(test)]
mod test {
    use crate::node::Node;
    use crate::treap::HashTreap;

    impl<'a> HashTreap<'a> {
        pub fn as_mermaid_graph(&self) -> String {
            let mut graph = String::new();

            graph.push_str("graph TD;\n");

            if let Some(root) = self.root.clone() {
                self.build_graph_string(&root, &mut graph);
            }

            graph.push_str(&format!(
                "    classDef null fill:#1111,stroke-width:1px,color:#fff,stroke-dasharray: 5 5;\n"
            ));

            graph
        }

        fn build_graph_string(&self, node: &Node, graph: &mut String) {
            let key = bytes_to_string(node.key());
            let node_label = format!("{}(({}))", node.hash(), key);

            // graph.push_str(&format!("## START node {}\n", node_label));
            if let Some(child) = self.get_node(node.left()) {
                let key = bytes_to_string(child.key());
                let child_label = format!("{}(({}))", child.hash(), key);

                graph.push_str(&format!("    {} --l--> {};\n", node_label, child_label));
                self.build_graph_string(&child, graph);
            } else {
                graph.push_str(&format!("    {} -.-> {}l((l));\n", node_label, node.hash()));
                graph.push_str(&format!("    class {}l null;\n", node.hash()));
            }
            // graph.push_str(&format!("## done left at node {}\n", node_label));

            if let Some(child) = self.get_node(node.right()) {
                let key = bytes_to_string(child.key());
                let child_label = format!("{}(({}))", child.hash(), key);

                graph.push_str(&format!("    {} --r--> {};\n", node_label, child_label));
                self.build_graph_string(&child, graph);
            } else {
                graph.push_str(&format!("    {} -.-> {}r((r));\n", node_label, node.hash()));
                graph.push_str(&format!("    class {}r null;\n", node.hash()));
            }
            // graph.push_str(&format!("## done right at node {}\n", node_label));
        }
    }

    fn bytes_to_string(byte: &[u8]) -> String {
        String::from_utf8(byte.to_vec()).expect("Invalid utf8 key in test with mermaig graph")
    }
}
