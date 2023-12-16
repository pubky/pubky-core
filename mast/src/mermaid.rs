use crate::treap::{Node, Treap};

impl Treap {
    pub fn as_mermaid_graph(&self) -> String {
        let mut graph = String::new();

        graph.push_str("graph TD;\n");

        if let Some(root) = self.get_node(self.root) {
            self.build_graph_string(&root, &mut graph);
        }

        graph
    }

    fn build_graph_string(&self, node: &Node, graph: &mut String) {
        let key = bytes_to_string(&node.key);
        let node_label = format!("{}({}:)", key, key);

        if let Some(left) = self.get_node(node.left) {
            let key = bytes_to_string(&left.key);
            let left_label = format!("{}({})", key, key);

            graph.push_str(&format!("    {} --> {};\n", node_label, left_label));
            self.build_graph_string(&left, graph);
        }

        if let Some(right) = self.get_node(node.right) {
            let key = bytes_to_string(&right.key);
            let right_label = format!("{}({})", key, key);

            graph.push_str(&format!("    {} --> {};\n", node_label, right_label));
            self.build_graph_string(&right, graph);
        }
    }
}

fn bytes_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b.to_string()).collect()
}
