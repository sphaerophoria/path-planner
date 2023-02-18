use serde::{Deserialize, Serialize};

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
