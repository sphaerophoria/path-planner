use anyhow::{anyhow, bail, Context, Result};
use common::{Data, Node, Way};
use glow::HasContext;
use regex::Regex;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};

macro_rules! define_gl_resource {
    ($name:ident, $resource_type:ty, $allocator:expr, $deleter:expr) => {
        struct $name {
            gl: Arc<glow::Context>,
            resource: $resource_type,
        }

        impl $name {
            fn new(gl: &Arc<glow::Context>) -> Result<$name, String> {
                let resource = unsafe { $allocator(&gl)? };
                Ok($name {
                    gl: Arc::clone(gl),
                    resource,
                })
            }
        }

        impl Deref for $name {
            type Target = $resource_type;
            fn deref(&self) -> &$resource_type {
                &self.resource
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                unsafe {
                    $deleter(&self.gl, self.resource);
                }
            }
        }
    };
}

define_gl_resource!(
    ScopedVertexArray,
    glow::VertexArray,
    glow::Context::create_vertex_array,
    glow::Context::delete_vertex_array
);
define_gl_resource!(
    ScopedBuffer,
    glow::Buffer,
    glow::Context::create_buffer,
    glow::Context::delete_buffer
);
define_gl_resource!(
    ScopedProgram,
    glow::Program,
    glow::Context::create_program,
    glow::Context::delete_program
);

define_gl_resource!(
    ScopedFramebuffer,
    glow::Framebuffer,
    glow::Context::create_framebuffer,
    glow::Context::delete_framebuffer
);
define_gl_resource!(
    ScopedRenderbuffer,
    glow::Renderbuffer,
    glow::Context::create_renderbuffer,
    glow::Context::delete_renderbuffer
);

struct ScopedGlEnable<'a> {
    gl: &'a glow::Context,
    prev_enabled: bool,
    flag: u32,
}

impl ScopedGlEnable<'_> {
    fn new(gl: &glow::Context, flag: u32) -> ScopedGlEnable {
        unsafe {
            let prev_enabled = gl.is_enabled(flag);
            gl.enable(flag);

            ScopedGlEnable {
                gl,
                prev_enabled,
                flag,
            }
        }
    }
}

impl Drop for ScopedGlEnable<'_> {
    fn drop(&mut self) {
        if !self.prev_enabled {
            unsafe {
                self.gl.disable(self.flag);
            }
        }
    }
}

const WAY_FINDER_RES: i32 = 11;

#[derive(Debug)]
pub struct PixelCoord {
    pub x: f32,
    pub y: f32,
}

pub struct PixelOffset {
    pub x: f32,
    pub y: f32,
}

pub struct Size {
    pub width: u32,
    pub height: u32,
}

pub struct GeoCoord {
    pub long: f32,
    pub lat: f32,
}

#[derive(Clone)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Color {
    pub fn from_rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b }
    }
}

#[derive(Clone)]
struct WayPosition {
    way_id: i32,
    node_id: usize,
    distance_to_next: f32,
}

impl Default for WayPosition {
    fn default() -> WayPosition {
        WayPosition {
            way_id: -1,
            node_id: 0,
            distance_to_next: 0.0,
        }
    }
}

#[repr(C, packed(1))]
struct VertexData {
    long: f32,
    lat: f32,
    way_id: i32,
    r: f32,
    g: f32,
    b: f32,
}

struct MapRenderer {
    gl: Arc<glow::Context>,
    vertex_array: ScopedVertexArray,
    _vertex_buffer: ScopedBuffer,
    _index_buffer: ScopedBuffer,
    index_buffer_length: i32,
    program: ScopedProgram,
    wayfinder_program: ScopedProgram,
    wayfinder_fbo: ScopedFramebuffer,
    _wayfinder_rbo: ScopedRenderbuffer,
    single_point_vertex_array: ScopedVertexArray,
    _single_point_vertex_buffer: ScopedBuffer,
}

impl MapRenderer {
    fn new(gl: Arc<glow::Context>, data: &Data) -> Result<MapRenderer> {
        assert_eq!(std::mem::size_of::<VertexData>(), 24);

        unsafe {
            let program = create_program(
                &gl,
                &[
                    (glow::VERTEX_SHADER, include_str!("map_vertex_shader.glsl")),
                    (
                        glow::FRAGMENT_SHADER,
                        include_str!("map_fragment_shader.glsl"),
                    ),
                ],
            )
            .context("Failed to create map renderer program")?;

            let vertex_array = ScopedVertexArray::new(&gl)
                .map_err(|s| anyhow!(s))
                .context("Failed to create map vertex array")?;
            gl.bind_vertex_array(Some(*vertex_array));

            let vertex_buffer = ScopedBuffer::new(&gl)
                .map_err(|s| anyhow!(s))
                .context("Failed to create map buffer")?;

            gl.bind_buffer(glow::ARRAY_BUFFER, Some(*vertex_buffer));

            let index_buffer = ScopedBuffer::new(&gl)
                .map_err(|s| anyhow!(s))
                .context("Failed to create map index buffer")?;
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(*index_buffer));

            let index_buffer_len = construct_bind_map_buffers(&gl, data, &[]);

            set_vertex_attrib_pointers(&gl, *program);

            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);

            let single_point_vertex_array = ScopedVertexArray::new(&gl)
                .map_err(|s| anyhow!(s))
                .context("Failed to create secondary vertex array")?;

            gl.bind_vertex_array(Some(*single_point_vertex_array));

            let single_point_vertex_buffer = ScopedBuffer::new(&gl)
                .map_err(|s| anyhow!(s))
                .context("Failed to create secondary vertex buffer")?;

            gl.bind_buffer(glow::ARRAY_BUFFER, Some(*single_point_vertex_buffer));

            set_vertex_attrib_pointers(&gl, *program);

            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);

            let wayfinder_program = create_program(
                &gl,
                &[
                    (glow::VERTEX_SHADER, include_str!("map_vertex_shader.glsl")),
                    (glow::FRAGMENT_SHADER, include_str!("color_way_id.glsl")),
                ],
            )
            .map_err(|s| anyhow!(s))
            .context("Failed to create wayfinder program")?;

            let wayfinder_rbo = ScopedRenderbuffer::new(&gl)
                .map_err(|s| anyhow!(s))
                .context("Failed to create wayfinder render buffer")?;
            gl.bind_renderbuffer(glow::RENDERBUFFER, Some(*wayfinder_rbo));
            gl.renderbuffer_storage(
                glow::RENDERBUFFER,
                glow::R32I,
                WAY_FINDER_RES,
                WAY_FINDER_RES,
            );

            let wayfinder_fbo = ScopedFramebuffer::new(&gl)
                .map_err(|s| anyhow!(s))
                .context("Failed to create wayfinder frame buffer")?;

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(*wayfinder_fbo));
            gl.framebuffer_renderbuffer(
                glow::DRAW_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::RENDERBUFFER,
                Some(*wayfinder_rbo),
            );

            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.bind_renderbuffer(glow::RENDERBUFFER, None);

            Ok(MapRenderer {
                gl,
                program,
                vertex_array,
                _vertex_buffer: vertex_buffer,
                _index_buffer: index_buffer,
                index_buffer_length: index_buffer_len as i32,
                wayfinder_program,
                wayfinder_fbo,
                _wayfinder_rbo: wayfinder_rbo,
                single_point_vertex_array,
                _single_point_vertex_buffer: single_point_vertex_buffer,
            })
        }
    }

    fn set_highlight_list(&self, data: &Data, highlights: &[(Regex, Color)]) {
        unsafe {
            self.gl.bind_vertex_array(Some(*self.vertex_array));
            self.gl
                .bind_buffer(glow::ARRAY_BUFFER, Some(*self._vertex_buffer));
            self.gl
                .bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(*self._index_buffer));
            construct_bind_map_buffers(&self.gl, data, highlights);
            self.gl.bind_vertex_array(None);
            self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
            self.gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, None);
        }
    }

    fn render(
        &self,
        scale: f32,
        center: &GeoCoord,
        aspect_ratio: f32,
        selected_way: i32,
        selected_position: Option<GeoCoord>,
        planned_path: &[GeoCoord],
        debug: bool,
    ) {
        unsafe {
            self.gl.use_program(Some(*self.program));

            let scale_loc = self
                .gl
                .get_uniform_location(*self.program, "scale")
                .unwrap();
            let center_loc = self
                .gl
                .get_uniform_location(*self.program, "center")
                .unwrap();
            let selected_way_loc = self
                .gl
                .get_uniform_location(*self.program, "selected_way")
                .unwrap();
            let aspect_ratio_loc = self
                .gl
                .get_uniform_location(*self.program, "aspect_ratio")
                .unwrap();

            self.gl.uniform_1_f32(Some(&scale_loc), scale);
            self.gl
                .uniform_2_f32(Some(&center_loc), center.long, center.lat);
            self.gl.uniform_1_f32(Some(&aspect_ratio_loc), aspect_ratio);
            self.gl.uniform_1_i32(Some(&selected_way_loc), selected_way);

            self.gl.bind_vertex_array(Some(*self.vertex_array));

            self.gl.clear_color(0.5, 0.5, 0.5, 1.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);

            self.gl.draw_elements(
                glow::LINE_STRIP,
                self.index_buffer_length,
                glow::UNSIGNED_INT,
                0,
            );

            self.gl.bind_vertex_array(None);

            if let Some(selected_position) = selected_position {
                self.gl
                    .bind_vertex_array(Some(*self.single_point_vertex_array));
                self.gl
                    .bind_buffer(glow::ARRAY_BUFFER, Some(*self._single_point_vertex_buffer));

                let vertex_buffer_data = VertexData {
                    lat: selected_position.lat,
                    long: selected_position.long,
                    way_id: selected_way,
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                };
                let vertex_buffer_u8 = std::slice::from_raw_parts(
                    &vertex_buffer_data as *const VertexData as *const u8,
                    std::mem::size_of::<VertexData>(),
                );
                self.gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    vertex_buffer_u8,
                    glow::STATIC_DRAW,
                );

                self.gl.draw_arrays(glow::POINTS, 0, 1);

                self.gl.bind_vertex_array(None);
                self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
            }

            if !planned_path.is_empty() {
                self.gl
                    .bind_vertex_array(Some(*self.single_point_vertex_array));
                self.gl
                    .bind_buffer(glow::ARRAY_BUFFER, Some(*self._single_point_vertex_buffer));

                let vertex_buffer_data: Vec<VertexData> = planned_path
                    .iter()
                    .map(|coord| VertexData {
                        lat: coord.lat,
                        long: coord.long,
                        way_id: -1,
                        r: 0.0,
                        g: 0.0,
                        b: 1.0,
                    })
                    .collect();

                let vertex_buffer_u8 = std::slice::from_raw_parts(
                    vertex_buffer_data.as_ptr() as *const u8,
                    vertex_buffer_data.len() * std::mem::size_of::<VertexData>(),
                );
                self.gl.buffer_data_u8_slice(
                    glow::ARRAY_BUFFER,
                    vertex_buffer_u8,
                    glow::STATIC_DRAW,
                );

                if debug {
                    self.gl
                        .draw_arrays(glow::POINTS, 0, vertex_buffer_data.len() as i32);
                } else {
                    self.gl
                        .draw_arrays(glow::LINE_STRIP, 0, vertex_buffer_data.len() as i32);
                }

                self.gl.bind_vertex_array(None);
                self.gl.bind_buffer(glow::ARRAY_BUFFER, None);
            }
        }
    }

    fn render_way_ids(&self, scale: f32, center: &GeoCoord) -> Vec<i32> {
        unsafe {
            self.gl.use_program(Some(*self.wayfinder_program));

            self.gl.viewport(0, 0, WAY_FINDER_RES, WAY_FINDER_RES);

            let scale_loc = self
                .gl
                .get_uniform_location(*self.wayfinder_program, "scale")
                .unwrap();
            let center_loc = self
                .gl
                .get_uniform_location(*self.wayfinder_program, "center")
                .unwrap();
            let aspect_ratio_loc = self
                .gl
                .get_uniform_location(*self.wayfinder_program, "aspect_ratio")
                .unwrap();

            self.gl.uniform_1_f32(Some(&scale_loc), scale);
            self.gl
                .uniform_2_f32(Some(&center_loc), center.long, center.lat);
            self.gl.uniform_1_f32(Some(&aspect_ratio_loc), 1.0);

            self.gl.bind_vertex_array(Some(*self.vertex_array));
            self.gl
                .bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(*self.wayfinder_fbo));

            self.gl
                .clear_buffer_i32_slice(glow::COLOR, 0, &[-1, -1, -1, -1]);

            self.gl.draw_elements(
                glow::LINE_STRIP,
                self.index_buffer_length,
                glow::UNSIGNED_INT,
                0,
            );

            #[repr(C, packed(1))]
            #[derive(Default, Debug, Clone, Copy)]
            struct Pixel {
                r: i32,
                g: i32,
                b: i32,
                a: i32,
            }

            let mut pixels = vec![Pixel::default(); (WAY_FINDER_RES * WAY_FINDER_RES) as usize];

            {
                self.gl
                    .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(*self.wayfinder_fbo));
                let pixel_slice = std::slice::from_raw_parts_mut(
                    pixels.as_mut_ptr() as *mut u8,
                    pixels.len() * std::mem::size_of::<Pixel>(),
                );
                self.gl.read_pixels(
                    0,
                    0,
                    WAY_FINDER_RES,
                    WAY_FINDER_RES,
                    glow::RGBA_INTEGER,
                    glow::INT,
                    glow::PixelPackData::Slice(pixel_slice),
                );
            }

            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.bind_vertex_array(None);
            pixels.into_iter().map(|v| v.r).collect()
        }
    }
}

struct PathPlanner {
    data: Arc<Data>,
    node_neighbors: Vec<Vec<usize>>,
}

impl PathPlanner {
    fn new(data: Arc<Data>) -> PathPlanner {
        let mut node_neighbors: Vec<HashSet<usize>> = vec![HashSet::new(); data.nodes.len()];

        for way in &data.ways {
            for (i, node_id) in way.nodes.iter().enumerate() {
                if i + 1 < way.nodes.len() {
                    node_neighbors[*node_id].insert(way.nodes[i + 1]);
                }

                if i > 0 {
                    node_neighbors[*node_id].insert(way.nodes[i - 1]);
                }
            }
        }

        let node_neighbors: Vec<Vec<usize>> = node_neighbors
            .into_iter()
            .map(|x| x.into_iter().collect())
            .collect();

        PathPlanner {
            data,
            node_neighbors,
        }
    }

    fn plan_path(&self, start_node: usize, end_node: usize, debug_paths: bool) -> Vec<GeoCoord> {
        #[derive(PartialEq)]
        struct Item {
            f_score: Reverse<f32>,
            item: usize,
        }

        impl Eq for Item {}

        impl PartialOrd for Item {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                self.f_score.partial_cmp(&other.f_score)
            }
        }

        impl Ord for Item {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.partial_cmp(other).expect("Invalid f score")
            }
        }

        #[derive(Clone)]
        struct Scores {
            g_score: f32,
            f_score: f32,
        }

        let mut open_set = BinaryHeap::new();
        open_set.push(Item {
            f_score: Reverse(0.0),
            item: start_node,
        });

        let mut came_from: HashMap<usize, usize> = HashMap::new();
        let mut scores = vec![
            Scores {
                g_score: f32::INFINITY,
                f_score: f32::INFINITY
            };
            self.data.nodes.len()
        ];
        scores[start_node].g_score = 0.0;
        scores[start_node].f_score =
            distance(&self.data.nodes[start_node], &self.data.nodes[end_node]);

        const MAX_ITERS: usize = 10000000;
        let mut i = 0;
        while let Some(item) = open_set.pop() {
            i += 1;

            if i >= MAX_ITERS {
                break;
            }

            let item = item.item;

            if item == end_node {
                if debug_paths {
                    break;
                } else {
                    return reconstruct_path(&self.data, &came_from, item);
                }
            }

            for neighbor in &self.node_neighbors[item] {
                let neighbor_distance =
                    distance(&self.data.nodes[item], &self.data.nodes[*neighbor]);
                let tentative_g_score = scores[item].g_score + neighbor_distance;

                if tentative_g_score < scores[*neighbor].g_score {
                    came_from.insert(*neighbor, item);
                    scores[*neighbor].g_score = tentative_g_score;
                    scores[*neighbor].f_score = tentative_g_score
                        + distance(&self.data.nodes[*neighbor], &self.data.nodes[end_node]);

                    open_set.push(Item {
                        f_score: Reverse(scores[*neighbor].f_score),
                        item: *neighbor,
                    });
                }
            }
        }

        if debug_paths {
            scores
                .iter()
                .enumerate()
                .filter_map(|(i, scores)| {
                    if scores.f_score < f32::INFINITY {
                        Some(i)
                    } else {
                        None
                    }
                })
                .map(|k: usize| node_to_geocoord(&self.data.nodes[k]))
                .collect()
        } else {
            Vec::new()
        }
    }
}

pub struct App {
    gl: Arc<glow::Context>,
    data: Arc<Data>,
    map_renderer: MapRenderer,
    path_planner: PathPlanner,
    path_start: WayPosition,
    planned_path: Vec<GeoCoord>,
    way_position: WayPosition,
    scale: f32,
    center: GeoCoord,
    debug: bool,
}

impl App {
    pub fn new(gl: Arc<glow::Context>, data: Data) -> Result<App> {
        let scale = 10.0;
        let center = GeoCoord {
            long: -123.153946,
            lat: 49.257828,
        };

        let mut tags = HashSet::new();
        for way in &data.ways {
            tags.extend(way.tags.iter());
        }

        let map_renderer =
            MapRenderer::new(Arc::clone(&gl), &data).context("Failed to create map renderer")?;
        let data = Arc::new(data);
        let path_planner = PathPlanner::new(Arc::clone(&data));

        Ok(App {
            gl,
            data,
            path_planner,
            path_start: Default::default(),
            planned_path: Vec::new(),
            map_renderer,
            scale,
            center,
            way_position: Default::default(),
            debug: false,
        })
    }

    /// Movement in pixel space, assuming the provided viewport dimensions
    pub fn move_map(&mut self, offset: &PixelOffset, viewport_size: &Size) {
        let center_pixel = PixelCoord {
            x: viewport_size.width as f32 / 2.0,
            y: viewport_size.height as f32 / 2.0,
        };

        let new_center_pixel = PixelCoord {
            x: center_pixel.x + offset.x,
            y: center_pixel.y + offset.y,
        };

        self.center = self.pixel_to_geocoord(&new_center_pixel, viewport_size);
    }

    pub fn set_debug_mode(&mut self, enable: bool) {
        self.debug = enable;
    }

    /// Change the zoom level. 2.0 sets the viewport such that the width of the viewport shows half
    /// the long that it used to. 0.5 sets the viewport such that the width of the viewport shows
    /// double the long that it used to
    pub fn zoom(&mut self, amount: f32, zoom_center: &PixelCoord, viewport_size: &Size) {
        let mouse_coord = self.pixel_to_geocoord(zoom_center, viewport_size);
        self.scale *= amount;

        let new_mouse_coord = self.pixel_to_geocoord(zoom_center, viewport_size);
        self.center.long -= new_mouse_coord.long - mouse_coord.long;
        self.center.lat -= new_mouse_coord.lat - mouse_coord.lat;
    }

    pub fn render_map(&self) {
        let _guards = setup_render(&self.gl);

        let aspect_ratio = unsafe {
            let mut viewport_dims = [0; 4];
            self.gl
                .get_parameter_i32_slice(glow::VIEWPORT, &mut viewport_dims);

            self.gl.scissor(
                viewport_dims[0],
                viewport_dims[1],
                viewport_dims[2],
                viewport_dims[3],
            );

            viewport_dims[2] as f32 / viewport_dims[3] as f32
        };

        let selected_geocoord = way_position_to_geocoord(&self.data, &self.way_position);
        self.map_renderer.render(
            self.scale,
            &self.center,
            aspect_ratio,
            self.way_position.way_id,
            selected_geocoord,
            &self.planned_path,
            self.debug,
        );
    }

    pub fn update_cursor_pos(&mut self, cursor_pos: Option<&PixelCoord>, viewport_size: &Size) {
        // Clone gl so that we can use mutable self later
        let gl_copy = Arc::clone(&self.gl);
        let _guards = setup_render(&gl_copy);

        self.update_selected_id(cursor_pos, viewport_size);

        if self.path_start.way_id != -1 && self.way_position.way_id != -1 {
            self.planned_path = self.path_planner.plan_path(
                self.data.ways[self.path_start.way_id as usize].nodes[self.path_start.node_id],
                self.data.ways[self.way_position.way_id as usize].nodes[self.way_position.node_id],
                self.debug,
            );
        } else {
            self.planned_path = Vec::new();
        }
    }

    pub fn pixel_to_geocoord(&self, pixel: &PixelCoord, viewport_size: &Size) -> GeoCoord {
        let x_rel = ((pixel.x / viewport_size.width as f32) * 2.0 - 1.0)
            * viewport_size.width as f32
            / viewport_size.height as f32;
        let y_rel = (1.0 - pixel.y / viewport_size.height as f32) * 2.0 - 1.0;

        // We also scale our image with our latitude so that our map has 1
        // degree x ~= 1 degree y in distance. We can be pretty approximate
        // here.
        let x_long_rel =
            x_rel / self.scale / f32::cos(self.center.lat * std::f32::consts::PI / 180.0);

        let y_lat_rel = y_rel / self.scale;

        GeoCoord {
            long: x_long_rel + self.center.long,
            lat: y_lat_rel + self.center.lat,
        }
    }

    pub fn selected_tags(&self) -> Vec<&str> {
        if self.way_position.way_id >= 0 {
            self.data.ways[self.way_position.way_id as usize]
                .tags
                .iter()
                .map(|s| s.as_ref())
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn start_path_plan(&mut self) {
        self.path_start = self.way_position.clone();
    }

    pub fn clear_path_plan(&mut self) {
        self.path_start = Default::default();
    }

    pub fn set_highlight_list(&self, highlights: &[(String, Color)]) -> Result<()> {
        let highlights = highlights
            .iter()
            .map(|(s, c)| {
                let r = Regex::new(s)?;
                Ok((r, c.clone()))
            })
            .collect::<Result<Vec<(Regex, Color)>>>()?;

        self.map_renderer
            .set_highlight_list(&self.data, &highlights);

        Ok(())
    }

    fn update_selected_id(&mut self, cursor_pos: Option<&PixelCoord>, viewport_size: &Size) {
        let cursor_pos = match cursor_pos {
            Some(v) => v,
            None => {
                self.way_position = Default::default();
                return;
            }
        };

        // This is what I would consider to be a colossal hack
        //
        // In order to figure out which "way" is closest to us, we don't really
        // have a choice but to check our proximity to every way. We could
        // pre-process this data to try to create "cells" of ways that reduce
        // the amount of checks, but there's a lot of complexity there.
        //
        // If we check every way in javascript, doing a simple axis aligned
        // bbox check, it takes 174ms. That's completely unreasonable if we
        // want to do it every frame or on every mouse move.
        //
        // The good news is we already render every single road on every frame,
        // and that's incredibly fast. The GPU is handling everything for us.
        // If we can somehow leverage that power in WebGL we get the answer
        // we're looking for.
        //
        // So we...
        // * Use our usual map drawing vertex shader, but we center the
        //   "viewport" on a very zoomed in segment near the provided longitude
        //   and latitude
        // * We use a special fragment shader that just outputs the way id
        //   directly
        // * We render that scene to a 11x11 pixel render buffer
        // * We iterate over the 121 pixels to find the way ID closest to the
        //   center
        // * If we don't find anything we zoom out and try again

        let mut scale = self.scale * 50.0;
        // Arbitrary cutoff
        // NOTE: The farther away from the cursor we get, the less accurate this becomes
        let cursor_coord_geo = self.pixel_to_geocoord(cursor_pos, viewport_size);
        while scale > 50.0 {
            let pixels = self.map_renderer.render_way_ids(scale, &cursor_coord_geo);

            let way_id = find_closest_way_id_to_center(&pixels);
            self.way_position = find_way_position(&self.data, way_id, &cursor_coord_geo);

            if self.way_position.way_id != -1 {
                break;
            }

            scale /= 2.0;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn setup_render(gl: &glow::Context) -> [ScopedGlEnable; 4] {
    [
        ScopedGlEnable::new(gl, glow::PRIMITIVE_RESTART),
        ScopedGlEnable::new(gl, glow::PRIMITIVE_RESTART_FIXED_INDEX),
        ScopedGlEnable::new(gl, glow::SCISSOR_TEST),
        ScopedGlEnable::new(gl, glow::PROGRAM_POINT_SIZE),
    ]
}

#[cfg(target_arch = "wasm32")]
fn setup_render(gl: &glow::Context) -> [ScopedGlEnable; 0] {
    []
}

fn create_program(gl: &Arc<glow::Context>, shaders: &[(u32, &str)]) -> Result<ScopedProgram> {
    unsafe {
        let program = ScopedProgram::new(gl)
            .map_err(|s| anyhow!(s))
            .context("Cannot create program")?;

        let shaders = shaders
            .iter()
            .map(|(shader_type, shader_source)| {
                let shader = gl
                    .create_shader(*shader_type)
                    .map_err(|s| anyhow!(s))
                    .context("Cannot create shader")?;
                gl.shader_source(shader, shader_source);
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    bail!(
                        "Failed to compile {shader_type}: {}",
                        gl.get_shader_info_log(shader)
                    );
                }
                gl.attach_shader(*program, shader);
                Ok(shader)
            })
            .collect::<Result<Vec<_>>>()
            .context("Failed to compile shaders")?;

        gl.link_program(*program);
        if !gl.get_program_link_status(*program) {
            bail!("{}", gl.get_program_info_log(*program));
        }

        for shader in shaders {
            gl.detach_shader(*program, shader);
            gl.delete_shader(shader);
        }

        Ok(program)
    }
}

fn set_vertex_attrib_pointers(gl: &glow::Context, program: glow::Program) {
    unsafe {
        let long_lat_loc = gl.get_attrib_location(program, "long_lat").unwrap();
        let way_id_loc = gl.get_attrib_location(program, "way_id").unwrap();
        let color_loc = gl.get_attrib_location(program, "v_color").unwrap();

        gl.vertex_attrib_pointer_f32(
            long_lat_loc,
            2,
            glow::FLOAT,
            false,
            std::mem::size_of::<VertexData>() as i32,
            0,
        );
        gl.enable_vertex_attrib_array(long_lat_loc);

        gl.vertex_attrib_pointer_i32(
            way_id_loc,
            1,
            glow::INT,
            std::mem::size_of::<VertexData>() as i32,
            8,
        );
        gl.enable_vertex_attrib_array(way_id_loc);

        gl.vertex_attrib_pointer_f32(
            color_loc,
            3,
            glow::FLOAT,
            false,
            std::mem::size_of::<VertexData>() as i32,
            12,
        );
        gl.enable_vertex_attrib_array(color_loc);
    }
}

fn pixel_from_buffer(pixels: &[i32], x: i32, y: i32) -> i32 {
    pixels[(y * WAY_FINDER_RES + x) as usize]
}

fn find_closest_way_id_to_center(pixels: &[i32]) -> i32 {
    let center_val = WAY_FINDER_RES / 2;

    for dist in 0..center_val + 1 {
        let lower_idx = center_val - dist;
        let higher_idx = center_val + dist;

        for i in lower_idx..=higher_idx {
            let way_ids = [
                pixel_from_buffer(pixels, i, lower_idx),
                pixel_from_buffer(pixels, i, higher_idx),
                pixel_from_buffer(pixels, lower_idx, i),
                pixel_from_buffer(pixels, higher_idx, i),
            ];

            for way_id in way_ids {
                if way_id != -1 {
                    return way_id;
                }
            }
        }
    }

    -1
}

fn find_way_position(data: &Data, way_id: i32, coord: &GeoCoord) -> WayPosition {
    // Step through the given way until we find the location closest to the given coord

    if way_id == -1 {
        return WayPosition::default();
    }

    let way_nodes = &data.ways[way_id as usize].nodes;

    let mut min_dist_2 = f32::INFINITY;
    let mut min_dist_node = 0;
    let mut min_dist_factor = 0.0;

    for (node_id, nodes) in way_nodes.windows(2).enumerate() {
        let &[n1, n2] = nodes else { unreachable!() };
        let n1_coords = node_to_geocoord(&data.nodes[n1]);
        let n2_coords = node_to_geocoord(&data.nodes[n2]);

        const I_STEPS: usize = 10;
        for i in 0..I_STEPS {
            let distance_in = i as f32 / I_STEPS as f32;
            let way_long = (n2_coords.long - n1_coords.long) * distance_in + n1_coords.long;
            let way_lat = (n2_coords.lat - n1_coords.lat) * distance_in + n1_coords.lat;

            let long_dist = way_long - coord.long;
            let lat_dist = way_lat - coord.lat;

            let dist_2 = long_dist * long_dist + lat_dist * lat_dist;

            if dist_2 < min_dist_2 {
                min_dist_2 = dist_2;
                min_dist_node = node_id;
                min_dist_factor = distance_in;
            }
        }
    }

    WayPosition {
        way_id,
        node_id: min_dist_node,
        distance_to_next: min_dist_factor,
    }
}

fn node_to_geocoord(node: &Node) -> GeoCoord {
    GeoCoord {
        long: (node.long as f64 / 10000000.0) as f32,
        lat: (node.lat as f64 / 10000000.0) as f32,
    }
}

fn way_position_to_geocoord(data: &Data, position: &WayPosition) -> Option<GeoCoord> {
    if position.way_id < 0 {
        return None;
    }

    let way = &data.ways[position.way_id as usize];
    let n1 = &data.nodes[way.nodes[position.node_id]];
    let n2 = &data.nodes[way.nodes[position.node_id + 1]];

    let coord1 = node_to_geocoord(n1);
    let coord2 = node_to_geocoord(n2);

    let long = (coord2.long - coord1.long) * position.distance_to_next + coord1.long;
    let lat = (coord2.lat - coord1.lat) * position.distance_to_next + coord1.lat;

    Some(GeoCoord { long, lat })
}

fn distance(n1: &Node, n2: &Node) -> f32 {
    let long_dist = n2.long - n1.long;
    let lat_dist = n2.lat - n1.lat;

    let mut long_dist =
        long_dist as f32 * f32::cos((n2.lat as f32) / 10000000.0 * std::f32::consts::PI / 180.0);

    long_dist /= 10000000.0;
    let lat_dist = lat_dist as f32 / 10000000.0;

    f32::sqrt(long_dist * long_dist + lat_dist * lat_dist)
}

fn reconstruct_path(
    data: &Data,
    came_from: &HashMap<usize, usize>,
    mut current: usize,
) -> Vec<GeoCoord> {
    let mut total_path = vec![node_to_geocoord(&data.nodes[current])];
    while came_from.contains_key(&current) {
        current = came_from[&current];
        total_path.push(node_to_geocoord(&data.nodes[current]))
    }

    total_path
}

fn way_color(way: &Way, highlights: &[(Regex, Color)]) -> Color {
    for (r, c) in highlights {
        for tag in &way.tags {
            if r.is_match(tag) {
                return c.clone();
            }
        }
    }

    Color::from_rgb(1.0, 1.0, 1.0)
}

fn construct_bind_map_buffers(
    gl: &glow::Context,
    data: &Data,
    highlights: &[(Regex, Color)],
) -> usize {
    let mut vertex_buffer_data = Vec::new();
    let mut index_buffer_data: Vec<u32> = Vec::new();
    for (i, way) in data.ways.iter().enumerate() {
        let color = way_color(way, highlights);

        for node_id in &way.nodes {
            let node = &data.nodes[*node_id];
            vertex_buffer_data.push(VertexData {
                long: node.long as f32 / 10000000.0,
                lat: node.lat as f32 / 10000000.0,
                way_id: i as i32,
                r: color.r,
                g: color.g,
                b: color.b,
            });
            index_buffer_data.push((vertex_buffer_data.len() - 1) as u32);
        }

        index_buffer_data.push(u32::max_value());
    }

    unsafe {
        let vertex_buffer_u8 = std::slice::from_raw_parts(
            vertex_buffer_data.as_ptr() as *const u8,
            vertex_buffer_data.len() * std::mem::size_of::<VertexData>(),
        );
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, vertex_buffer_u8, glow::STATIC_DRAW);

        let index_buffer_u8 = std::slice::from_raw_parts(
            index_buffer_data.as_ptr() as *const u8,
            index_buffer_data.len() * std::mem::size_of::<u32>(),
        );
        gl.buffer_data_u8_slice(
            glow::ELEMENT_ARRAY_BUFFER,
            index_buffer_u8,
            glow::STATIC_DRAW,
        );
    }

    index_buffer_data.len()
}
