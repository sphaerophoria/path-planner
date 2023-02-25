use common::{Data, Node, Way};
use elevation_data::ElevationData;
use osmpbf::Element;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::{borrow::Cow, error::Error as StdError, fmt, fs::OpenOptions, path::PathBuf};

mod elevation_data;

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

pub fn data_from_osm_pbf<R>(pbf: R, elevation_data: &ElevationData) -> Result<Data, Error>
where
    R: std::io::Read + Send,
{
    let pbf_reader = osmpbf::ElementReader::new(pbf);

    let mut nodes = HashMap::new();
    let mut relevant_nodes = HashSet::new();
    let mut ways = Vec::new();
    pbf_reader
        .for_each(|elem| match elem {
            Element::Node(node) => {
                let lat = node.decimicro_lat();
                let long = node.decimicro_lon();
                let height = elevation_data
                    .height_at_lat_long(lat as f32 / 10000000.0, long as f32 / 10000000.0);
                nodes.insert(node.id(), Node { lat, long, height });
            }
            Element::DenseNode(node) => {
                let lat = node.decimicro_lat();
                let long = node.decimicro_lon();
                let height = elevation_data
                    .height_at_lat_long(lat as f32 / 10000000.0, long as f32 / 10000000.0);
                nodes.insert(node.id(), Node { lat, long, height });
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

#[derive(Debug)]
enum ArgParseError {
    InvalidArgument(String),
    MissingArgument(&'static str),
    MissingValue(&'static str),
}

impl fmt::Display for ArgParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ArgParseError::*;
        match self {
            InvalidArgument(s) => write!(f, "Invalid argument: {s}")?,
            MissingArgument(s) => write!(f, "Missing argument: {s}")?,
            MissingValue(s) => write!(f, "Missing value for {s}")?,
        };

        write!(f, "\n\n{}", Args::help())
    }
}

impl StdError for ArgParseError {}

struct Args {
    pbf_path: PathBuf,
    elevation_path: PathBuf,
    www_path: PathBuf,
}

impl Args {
    const ELEVATION_LONG_ARG: &str = "--elevation-path";
    const ELEVATION_SHORT_ARG: &str = "-e";
    const WWW_LONG_ARG: &str = "--www-path";
    const WWW_SHORT_ARG: &str = "-w";
    const OSM_LONG_ARG: &str = "--osm-pbf-path";
    const OSM_SHORT_ARG: &str = "-p";

    fn new<T, U>(inputs: T) -> Result<Args, ArgParseError>
    where
        T: IntoIterator<Item = U>,
        U: AsRef<str>,
    {
        use ArgParseError as E;
        let mut it = inputs.into_iter();
        // Skip exe name
        it.next();

        enum ArgData {
            Www(PathBuf),
            Osm(PathBuf),
            Elevation(PathBuf),
            Help,
            None,
        }

        impl ArgData {
            fn try_from<T, U>(mut it: T) -> Result<Self, ArgParseError>
            where
                T: Iterator<Item = U>,
                U: AsRef<str>,
            {
                let input = match it.next() {
                    Some(v) => v,
                    None => return Ok(ArgData::None),
                };

                let input = input.as_ref();

                match input {
                    Args::ELEVATION_LONG_ARG | Args::ELEVATION_SHORT_ARG => {
                        let val = it
                            .next()
                            .ok_or(ArgParseError::MissingValue(Args::ELEVATION_LONG_ARG))?;
                        Ok(ArgData::Elevation(val.as_ref().into()))
                    }
                    Args::OSM_LONG_ARG | Args::OSM_SHORT_ARG => {
                        let val = it
                            .next()
                            .ok_or(ArgParseError::MissingValue(Args::OSM_LONG_ARG))?;
                        Ok(ArgData::Osm(val.as_ref().into()))
                    }
                    Args::WWW_LONG_ARG | Args::WWW_SHORT_ARG => {
                        let val = it
                            .next()
                            .ok_or(ArgParseError::MissingValue(Args::WWW_LONG_ARG))?;
                        Ok(ArgData::Www(val.as_ref().into()))
                    }
                    "--help" => Ok(ArgData::Help),
                    a => Err(ArgParseError::InvalidArgument(a.into())),
                }
            }
        }

        let mut www_path = None;
        let mut pbf_path = None;
        let mut elevation_path = None;
        loop {
            match ArgData::try_from(&mut it)? {
                ArgData::Osm(p) => pbf_path = Some(p),
                ArgData::Elevation(p) => elevation_path = Some(p),
                ArgData::Www(p) => www_path = Some(p),
                ArgData::Help => {
                    eprintln!("{}", Args::help());
                    std::process::exit(0);
                }
                ArgData::None => break,
            }
        }

        macro_rules! unwrap_arg {
            ($val:expr, $name:expr) => {
                match $val {
                    Some(v) => v,
                    None => return Err(E::MissingArgument($name)),
                }
            };
        }

        let www_path = unwrap_arg!(www_path, Self::WWW_LONG_ARG);
        let pbf_path = unwrap_arg!(pbf_path, Self::OSM_LONG_ARG);
        let elevation_path = unwrap_arg!(elevation_path, Self::ELEVATION_LONG_ARG);

        Ok(Args {
            www_path,
            pbf_path,
            elevation_path,
        })
    }

    fn help() -> String {
        let exe_name = match env::current_exe() {
            Ok(v) => v,
            Err(_) => "server".into(),
        };

        format!(
            " \n\
                  {exe_name} \n\
                  \n\
                  Periodically produce an updated data.json for path-planner \n\
                  \n\
                  Args: \n\
                  \n\
                  {www_long} | {www_short} <WWW_PATH>: Where to write the output\n\
                  {elevation_long} | {elevation_short} <ELEVATION_PATH>: Where to read elevation data from\n\
                  {pbf_long} | {pbf_short} <PBF_PATH>: Where to read pbf data from\n\
                  "
        , exe_name=exe_name.display()
        , www_long=Self::WWW_LONG_ARG
        , www_short=Self::WWW_SHORT_ARG
        , elevation_long=Self::ELEVATION_LONG_ARG
        , elevation_short=Self::ELEVATION_SHORT_ARG
        , pbf_long=Self::OSM_LONG_ARG
        , pbf_short=Self::OSM_SHORT_ARG)
    }
}

fn main() -> Result<(), Error> {
    let args =
        Args::new(std::env::args()).map_err(|e| Error::new("Failed to parse arguments", e))?;

    let elevation_file = File::open(args.elevation_path)
        .map_err(|e| Error::new("Failed to open elevation file", e))?;
    let elevation_data = elevation_data::parse_elevation_data(BufReader::new(elevation_file))
        .map_err(|e| Error::new("Failed to parse elevation data", e))?;

    let pbf_file =
        File::open(args.pbf_path).map_err(|e| Error::new("Failed to open pbf file", e))?;
    let data = data_from_osm_pbf(BufReader::new(pbf_file), &elevation_data)
        .map_err(|e| Error::new("Failed to retrieve data", e))?;

    let output_path = args.www_path.join("data.json");
    let f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&output_path)
        .map_err(|e| Error::new(format!("Failed to open to {}", output_path.display()), e))?;

    let f = BufWriter::new(f);

    serde_json::to_writer(f, &data).map_err(|e| Error::new("Failed to serialize data", e))?;

    Ok(())
}
