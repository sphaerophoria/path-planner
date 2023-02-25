#![allow(unused)]
use std::{
    error::Error,
    fmt,
    io::{self, BufRead},
};

#[derive(Debug)]
pub struct Point {
    x: f32,
    y: f32,
}

pub struct ElevationData {
    step: f32,
    row_length: usize,
    tl_corner: Point,
    nodata_val: f32,
    data: Vec<f32>,
}

impl ElevationData {
    pub fn height_at_lat_long(&self, lat: f32, long: f32) -> Option<f32> {
        let lat_rel_tl_corner = self.tl_corner.y - lat;
        let long_rel_tl_corner = long - self.tl_corner.x;

        let x_idx = ((long_rel_tl_corner + self.step / 2.0) / self.step) as usize;
        let y_idx = ((lat_rel_tl_corner + self.step / 2.0) / self.step) as usize;

        if x_idx >= self.row_length {
            return None;
        }

        let idx = y_idx * self.row_length + x_idx;

        if idx >= self.data.len() {
            return None;
        }

        let ret = self.data[idx];

        if (f32::abs(ret - self.nodata_val) < 0.001) {
            return None;
        }

        Some(ret)
    }
}

#[derive(Debug)]
struct Header {
    rows: usize,
    cols: usize,
    xllcorner: f32,
    yllcorner: f32,
    cellsize: f32,
    nodata: f32,
}

impl TryFrom<HeaderBuilder> for Header {
    type Error = HeaderParseError;

    fn try_from(value: HeaderBuilder) -> Result<Self, Self::Error> {
        Ok(Header {
            rows: value.rows.ok_or(HeaderParseError::MissingFields("nrows"))?,
            cols: value.cols.ok_or(HeaderParseError::MissingFields("ncols"))?,
            xllcorner: value
                .xllcorner
                .ok_or(HeaderParseError::MissingFields("xllcorner"))?,
            yllcorner: value
                .yllcorner
                .ok_or(HeaderParseError::MissingFields("yllcorner"))?,
            cellsize: value
                .cellsize
                .ok_or(HeaderParseError::MissingFields("cellsize"))?,
            nodata: value
                .nodata
                .ok_or(HeaderParseError::MissingFields("nodata_value"))?,
        })
    }
}

#[derive(Default)]
struct HeaderBuilder {
    rows: Option<usize>,
    cols: Option<usize>,
    xllcorner: Option<f32>,
    yllcorner: Option<f32>,
    cellsize: Option<f32>,
    nodata: Option<f32>,
}

impl HeaderBuilder {
    fn ready(&self) -> bool {
        self.rows.is_some()
            && self.cols.is_some()
            && self.xllcorner.is_some()
            && self.yllcorner.is_some()
            && self.cellsize.is_some()
            && self.nodata.is_some()
    }
}

#[derive(Debug)]
pub enum HeaderParseError {
    Io(io::Error),
    MissingKey(usize),
    MissingValue(usize),
    InvalidKey(String),
    InvalidInt(std::num::ParseIntError),
    InvalidFloat(std::num::ParseFloatError),
    MissingFields(&'static str),
    ExtraHeaderValue(usize),
}

impl fmt::Display for HeaderParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use HeaderParseError::*;
        match self {
            Io(_) => write!(f, "Failed to read header data from file"),
            MissingKey(line) => write!(f, "Missing header key, line {line}"),
            MissingValue(line) => write!(f, "Missing header value, line {line}"),
            InvalidKey(value) => write!(f, "Invalid header key: {value}"),
            InvalidInt(_) => write!(f, "Invalid header value"),
            InvalidFloat(_) => write!(f, "Invalid header value"),
            MissingFields(field) => write!(
                f,
                "Not all required header fields are provided, missing: {field}"
            ),
            ExtraHeaderValue(line) => write!(f, "Header contained too much data, line {line}"),
        }
    }
}

impl Error for HeaderParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        use HeaderParseError::*;
        match self {
            Io(s) => Some(s),
            InvalidInt(s) => Some(s),
            InvalidFloat(s) => Some(s),
            MissingKey(_) | MissingValue(_) | InvalidKey(_) | ExtraHeaderValue(_)
            | MissingFields(_) => None,
        }
    }
}

fn parse_header<T>(buf: &mut std::io::Lines<T>) -> Result<Header, HeaderParseError>
where
    T: BufRead,
{
    let mut ret = HeaderBuilder::default();

    // Peek it so that we do not accidentally pull the data line
    let mut it = buf.enumerate().peekable();
    while let Some(item) = it.peek() {
        let line = match &item.1 {
            Ok(v) => v,
            // Have to actually consume the value to return it
            Err(e) => return Err(HeaderParseError::Io(it.next().unwrap().1.unwrap_err())),
        };

        let mut line_it = line.split_whitespace();

        let key = line_it.next();
        let value = line_it.next();

        if let Some(v) = line_it.next() {
            return Err(HeaderParseError::ExtraHeaderValue(item.0));
        }

        let key = match key {
            Some(v) => v.to_lowercase(),
            None => return Err(HeaderParseError::MissingKey(item.0)),
        };

        let value = match value {
            Some(v) => v.to_lowercase(),
            None => return Err(HeaderParseError::MissingValue(item.0)),
        };

        match key.as_str() {
            "ncols" => ret.cols = Some(value.parse().map_err(HeaderParseError::InvalidInt)?),
            "nrows" => ret.rows = Some(value.parse().map_err(HeaderParseError::InvalidInt)?),
            "xllcorner" => {
                ret.xllcorner = Some(value.parse().map_err(HeaderParseError::InvalidFloat)?)
            }
            "yllcorner" => {
                ret.yllcorner = Some(value.parse().map_err(HeaderParseError::InvalidFloat)?)
            }
            "cellsize" => {
                ret.cellsize = Some(value.parse().map_err(HeaderParseError::InvalidFloat)?)
            }
            "nodata_value" => {
                ret.nodata = Some(value.parse().map_err(HeaderParseError::InvalidFloat)?)
            }
            _ => return Err(HeaderParseError::InvalidKey(key)),
        }

        // Once we know we parsed correctly, we can increment the iterator for real
        it.next();
        if ret.ready() {
            break;
        }
    }

    ret.try_into()
}

#[derive(Debug)]
pub enum ElevationParseError {
    HeaderParse(HeaderParseError),
    Io(io::Error),
    InvalidFloat(std::num::ParseFloatError),
    InvalidDataSize {
        expected_rows: usize,
        expected_cols: usize,
        actual_size: usize,
    },
}

impl fmt::Display for ElevationParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ElevationParseError::*;
        match self {
            HeaderParse(_) => write!(f, "Failed to parse header"),
            Io(_) => write!(f, "Failed to read data"),
            InvalidFloat(_) => write!(f, "Invalid float in data"),
            InvalidDataSize {
                expected_rows,
                expected_cols,
                actual_size,
            } => {
                write!(f, "Invalid data size. Expected {expected_rows}x{expected_cols}, got {actual_size}")
            }
        }
    }
}
impl Error for ElevationParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        use ElevationParseError::*;
        match self {
            HeaderParse(s) => Some(s),
            Io(s) => Some(s),
            InvalidFloat(s) => Some(s),
            InvalidDataSize { .. } => None,
        }
    }
}

// Ersi grid https://en.wikipedia.org/wiki/Esri_grid
pub fn parse_elevation_data<T>(buf: T) -> Result<ElevationData, ElevationParseError>
where
    T: BufRead,
{
    let mut line_iter = buf.lines();
    let header = parse_header(&mut line_iter).map_err(ElevationParseError::HeaderParse)?;

    let mut data = Vec::with_capacity(header.rows * header.cols);
    for line in line_iter {
        let line = line.map_err(ElevationParseError::Io)?;
        for val in line.split_whitespace() {
            let val = val
                .parse::<f32>()
                .map_err(ElevationParseError::InvalidFloat)?;
            data.push(val);
        }
    }

    if data.len() != header.rows * header.cols {
        return Err(ElevationParseError::InvalidDataSize {
            expected_rows: header.rows,
            expected_cols: header.cols,
            actual_size: data.len(),
        });
    }

    let ret = ElevationData {
        step: header.cellsize,
        row_length: header.cols,
        tl_corner: Point {
            x: header.xllcorner,
            y: header.yllcorner + header.cellsize * header.rows as f32,
        },
        nodata_val: header.nodata,
        data,
    };

    Ok(ret)
}
