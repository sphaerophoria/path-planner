mod data;

use clap::Parser;
use std::{fs::OpenOptions, path::PathBuf};

const VANCOUVER: &[u8] = include_bytes!("../res/vancouver.osm.pbf");

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long)]
    www_path: PathBuf,
}

fn main() {
    let args = Args::parse();

    let data = data::Data::from_osm_pbf(VANCOUVER).unwrap();

    let f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(args.www_path.join("data.json"))
        .unwrap();

    serde_json::to_writer(f, &data).unwrap();
}
