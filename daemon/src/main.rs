use common::{Data, Node, Way};
use osmpbf::Element;
use std::collections::{HashMap, HashSet};
use std::{borrow::Cow, error::Error as StdError, fmt, fs::OpenOptions, path::PathBuf};

const VANCOUVER: &[u8] = include_bytes!("../res/vancouver.osm.pbf");

pub struct Error {
    reason: Cow<'static, str>,
    source: Option<Box<dyn StdError>>,
}

impl Error {
    fn new<S, E>(reason: S, source: E) -> Error
    where
        S: Into<Cow<'static, str>>,
        E: Into<Box<dyn StdError>>,
    {
        Error {
            reason: reason.into(),
            source: Some(source.into()),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.reason)?;

        let mut it = match self.source() {
            Some(e) => e,
            None => return Ok(()),
        };

        write!(f, "\n\nCaused by:\n{it}")?;

        while let Some(e) = it.source() {
            fmt::Display::fmt(e, f)?;
            it = e;
        }
        Ok(())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.source.as_deref()
    }
}

pub fn data_from_osm_pbf(pbf: &[u8]) -> Result<Data, Error> {
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
                let tag_keys = way.tags().map(|(k, _)| k).collect::<Vec<_>>();
                if !tag_keys.contains(&"highway") {
                    return;
                }
                for (key, value) in way.tags() {
                    if node_ids.is_empty() {
                        for id in way.refs() {
                            node_ids.push(id);
                        }

                        relevant_nodes.extend(node_ids.clone());
                    }

                    tags.push(format!("{key}/{value}"));
                }
                ways.push((node_ids, tags));
            }
            Element::Relation(_relation) => {}
        })
        .map_err(|e| Error::new("Failed to read osm pbf", e))?;

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

struct Args {
    www_path: PathBuf,
}

impl Args {
    fn new<T, U>(inputs: T) -> Args
    where
        T: IntoIterator<Item = U>,
        U: AsRef<str>,
    {
        let mut it = inputs.into_iter();
        let exe_name = it.next();
        let exe_name = exe_name.as_ref().map(|v| v.as_ref()).unwrap_or("server");

        let mut www_path = None;
        while let Some(input) = it.next() {
            let input = input.as_ref();
            match input {
                "--www-path" | "-w" => {
                    www_path = it.next();
                }
                "--help" => {
                    Self::help(exe_name);
                }
                a => {
                    eprintln!("Unknown argument: {a}");
                    Self::help(exe_name);
                }
            }
        }

        if www_path.is_none() {
            Self::help(exe_name);
        }

        let www_path = match www_path {
            Some(v) => v,
            None => Self::help(exe_name),
        };

        Args {
            www_path: www_path.as_ref().into(),
        }
    }

    fn help(exe_name: &str) -> ! {
        eprintln!(
            " \n\
                  {exe_name} \n\
                  \n\
                  Periodically produce an updated data.json for path-planner use \n\
                  \n\
                  Args: \n\
                  \n\
                  --www-path | -w <WWW_PATH>: Where to write the output \n"
        );

        std::process::exit(1);
    }
}

fn main() -> Result<(), Error> {
    let args = Args::new(std::env::args());

    let data =
        data_from_osm_pbf(VANCOUVER).map_err(|e| Error::new("Failed to retrieve data", e))?;

    let output_path = args.www_path.join("data.json");
    let f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&output_path)
        .map_err(|e| Error::new(format!("Failed to open to {}", output_path.display()), e))?;

    serde_json::to_writer(f, &data).map_err(|e| Error::new("Failed to serialize data", e))?;

    Ok(())
}
