use anyhow::{Context, Result};
use osmpbf::Element;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Serialize, Deserialize)]
pub struct Node {
    pub lat: i32,
    pub long: i32,
}

#[derive(Serialize, Deserialize)]
pub struct Way {
    pub tags: Vec<String>,
    pub nodes: Vec<usize>,
}

#[derive(Serialize, Deserialize)]
pub struct Data {
    pub nodes: Vec<Node>,
    pub ways: Vec<Way>,
}

impl Data {
    pub fn from_osm_pbf(pbf: &[u8]) -> Result<Data> {
        let pbf_reader = osmpbf::ElementReader::new(pbf);

        let mut nodes = HashMap::new();
        let mut relevant_nodes = HashSet::new();
        let mut ways = Vec::new();

        pbf_reader
            .for_each(|elem| match elem {
                Element::Node(node) => {
                    nodes.insert(
                        node.id(),
                        Node {
                            lat: node.decimicro_lat(),
                            long: node.decimicro_lon(),
                        },
                    );
                }
                Element::DenseNode(node) => {
                    nodes.insert(
                        node.id(),
                        Node {
                            lat: node.decimicro_lat(),
                            long: node.decimicro_lon(),
                        },
                    );
                }
                Element::Way(way) => {
                    let mut tags = Vec::new();
                    let mut node_ids = Vec::new();
                    for (key, value) in way.tags() {
                        if node_ids.is_empty() {
                            for id in way.refs() {
                                node_ids.push(id);
                            }

                            relevant_nodes.extend(node_ids.clone());
                        }

                        tags.push(format!("{}/{}", key, value));
                    }
                    ways.push((node_ids, tags));
                }
                Element::Relation(_relation) => {}
            })
            .context("Failed to read pbf")?;

        // Once we've walked the whole pbf, we can discard any nodes that are not related to our
        // paths. Since this will end up being a subset of all ids, we also heal the way references
        // to be indexes into a linear array of nodes. This has the nice side effect of simplifying
        // some rendering code. We can just upload this array to the GPU in a vertex buffer and use
        // the healed node ids as our index buffer
        let (node_mapping, nodes): (HashMap<i64, usize>, Vec<Node>) = nodes
            .into_iter()
            .filter(|(k, _)| relevant_nodes.contains(k))
            .enumerate()
            .map(|(i, (k, v))| ((k, i), v))
            .unzip();

        let mut new_ways = Vec::new();
        for way in ways.into_iter() {
            new_ways.push(Way {
                nodes: way.0.iter().map(|id| node_mapping[id]).collect(),
                tags: way.1,
            });
        }

        Ok(Data {
            nodes,
            ways: new_ways,
        })
    }
}
