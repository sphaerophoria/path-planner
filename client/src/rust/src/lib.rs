use serde::{Serialize, Deserialize};
use std::{
    cmp::Reverse,
    collections::{HashMap, BinaryHeap}
};
use wasm_bindgen::prelude::*;

#[derive(Deserialize)]
struct Node {
    lat: i32,
    long: i32,
}

#[derive(Deserialize)]
struct Way {
    nodes: Vec<usize>,
    tags: Vec<String>
}

#[derive(Deserialize)]
struct Data {
    nodes: Vec<Node>,
    ways: Vec<Way>,
    node_to_way: Vec<Vec<Vec<usize>>>
}

#[derive(Serialize)]
struct PlannedPath(Vec<(f32, f32)>);


#[wasm_bindgen]
pub fn init() {
    wasm_logger::init(Default::default());
}


#[wasm_bindgen]
pub struct PathPlanner {
    data: Data,
}

#[wasm_bindgen]
impl PathPlanner {
    #[wasm_bindgen(constructor)]
    pub fn new(data: JsValue) -> std::result::Result<PathPlanner, JsValue> {
        let data: Data = serde_wasm_bindgen::from_value(data)?;
        Ok(PathPlanner {
            data,
        })
    }

    #[wasm_bindgen]
    pub fn plan_path(&self, start: &[usize], end: &[usize], debug_paths: bool) -> JsValue {
        assert_eq!(start.len(), 3);
        assert_eq!(end.len(), 3);

        let start_node = self.data.ways[start[0]].nodes[start[1]];
        let end_node = self.data.ways[end[0]].nodes[end[1]];

        #[derive(PartialOrd, PartialEq)]
        struct Item {
            f_score: Reverse<f32>, 
            item: usize,
        }

        impl Eq for Item {}

        impl Ord for Item {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.partial_cmp(other).unwrap()
            }
        }

        #[derive(Clone)]
        struct Scores {
            g_score: f32,
            f_score: f32,
        }

        let mut open_set = BinaryHeap::new();
        open_set.push(Item { f_score: Reverse(0.0), item: start_node});

        let mut came_from: HashMap<usize, usize> = HashMap::new();
        let mut scores = vec![Scores {g_score: f32::INFINITY, f_score: f32::INFINITY}; self.data.nodes.len()];
        scores[start_node].g_score = 0.0;
        scores[start_node].f_score = distance(&self.data.nodes[start_node], &self.data.nodes[end_node]);


        const MAX_ITERS: usize = 10000000;
        let mut i = 0;
        while !open_set.is_empty() {
            i += 1;

            if i >= MAX_ITERS {
                break;
            }
            let item = open_set.pop().unwrap().item;

            if item == end_node {
                if debug_paths {
                    break;
                } else {
                    return serde_wasm_bindgen::to_value(&PlannedPath(reconstruct_path(&self.data, &came_from, item))).unwrap()
                }
            }

            for neighbor in neighbors(&self.data, item) {
                let neighbor_distance = distance(&self.data.nodes[item], &self.data.nodes[neighbor]);
                let tentative_g_score = scores[item].g_score + neighbor_distance;

                if tentative_g_score < scores[neighbor].g_score {
                    came_from.insert(neighbor, item);
                    scores[neighbor].g_score = tentative_g_score;
                    scores[neighbor].f_score =  tentative_g_score + distance(&self.data.nodes[neighbor], &self.data.nodes[end_node]);

                    open_set.push(Item {f_score: Reverse(scores[neighbor].f_score), item: neighbor});
                }

            }
        }

        if debug_paths {
            let ret = scores.iter().enumerate()
                .filter_map(|(i, scores)| {
                    if scores.f_score < f32::INFINITY {
                        Some(i)
                    }
                    else {
                        None
                    }
                })
                .map(|k: usize| node_to_long_lat(&self.data.nodes[k]))
                .collect();

            serde_wasm_bindgen::to_value(&PlannedPath(ret)).unwrap()
        } else {
            JsValue::NULL
        }
    }
}


fn node_to_long_lat(node: &Node) -> (f32, f32) {
    return (node.long as f32 / 10000000.0, node.lat as f32 / 10000000.0)
}

fn neighbors(data: &Data, item: usize)  -> Vec<usize> {
    let mut neighbors = Vec::new();

    for node_to_way in &data.node_to_way[item] {
        let way_id = &node_to_way[0];
        let sub_id = &node_to_way[1];
        let way = &data.ways[*way_id];

        if sub_id + 1 != way.nodes.len() {
            neighbors.push(way.nodes[sub_id + 1]);
        }

        if *sub_id != 0 {
            neighbors.push(way.nodes[sub_id - 1]);
        }
    }

    neighbors
}

fn distance(n1: &Node, n2: &Node) -> f32 {
    let long_dist = n2.long - n1.long;
    let lat_dist = n2.lat - n1.lat;

    let mut long_dist = long_dist as f32 * f32::cos((n2.lat as f32) / 10000000.0 * std::f32::consts::PI / 180.0);

    long_dist = long_dist / 10000000.0;
    let lat_dist = lat_dist as f32 / 10000000.0;

    f32::sqrt(long_dist * long_dist + lat_dist * lat_dist)
}

fn reconstruct_path(data: &Data, came_from: &HashMap<usize, usize>, mut current: usize) -> Vec<(f32, f32)> {
    let mut total_path = vec![node_to_long_lat(&data.nodes[current])];
    while came_from.contains_key(&current) {
        current = came_from[&current];
        total_path.push(node_to_long_lat(&data.nodes[current]))
    }

    total_path
}
