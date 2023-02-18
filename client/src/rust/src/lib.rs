use glow::HasContext;
use path_planner::{Color, PixelCoord, PixelOffset, Size};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn init() {
    wasm_logger::init(Default::default());
}

#[wasm_bindgen]
pub struct App {
    inner: path_planner::App,
    canvas: web_sys::HtmlCanvasElement,
    gl: Arc<glow::Context>,
}

#[wasm_bindgen]
impl App {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: JsValue, data: JsValue) -> std::result::Result<App, JsValue> {
        let canvas: web_sys::HtmlCanvasElement = canvas
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .map_err(|_| ())
            .unwrap();

        let webgl2_context = canvas
            .get_context("webgl2")
            .unwrap()
            .unwrap()
            .dyn_into::<web_sys::WebGl2RenderingContext>()
            .unwrap();

        let gl = Arc::new(glow::Context::from_webgl2_context(webgl2_context));

        let data = serde_wasm_bindgen::from_value(data).unwrap();
        let inner = path_planner::App::new(Arc::clone(&gl), data)
            .map_err(|e| JsValue::from_str(&format!("{:?}", e)))?;

        Ok(App {
            inner,
            canvas,
            gl,
        })
    }

    pub fn update_pointer_pos(&mut self, x: f32, y: f32) {
        let pixel_coord = PixelCoord { x, y };

        self.inner
            .update_cursor_pos(Some(&pixel_coord), &self.viewport_size());
    }

    pub fn zoom(&mut self, amount: f32, x: f32, y: f32) {
        let zoom_center = PixelCoord { x, y };

        self.inner.zoom(amount, &zoom_center, &self.viewport_size());
    }

    pub fn render(&self) {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        let body = document.body().unwrap();
        self.canvas.set_width(body.client_width() as u32);
        self.canvas.set_height(body.client_height() as u32);
        unsafe {
            self.gl.viewport(
                0,
                0,
                self.canvas.width() as i32,
                self.canvas.height() as i32,
            );
        }
        self.inner.render_map();
    }

    pub fn move_map(&mut self, dx: f32, dy: f32) {
        let movement = PixelOffset { x: dx, y: dy };

        self.inner.move_map(&movement, &self.viewport_size());
    }

    pub fn pixel_to_geocoord(&self, x: f32, y: f32) -> Vec<f32> {
        let coord = self
            .inner
            .pixel_to_geocoord(&PixelCoord { x, y }, &self.viewport_size());
        vec![coord.long, coord.lat]
    }

    pub fn set_debug_mode(&mut self, enable: bool) {
        self.inner.set_debug_mode(enable);
    }

    pub fn start_path_plan(&mut self) {
        self.inner.start_path_plan();
    }

    pub fn clear_path_plan(&mut self) {
        self.inner.clear_path_plan();
    }

    pub fn selected_tags(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.inner.selected_tags()).unwrap()
    }

    pub fn update_highlight(&self, regex: String, color: &[f32]) {
        let color = Color::from_rgb(color[0], color[1], color[2]);
        self.inner.set_highlight_list(&[
            (regex, color)
        ]);

    }

    fn viewport_size(&self) -> Size {
        Size {
            width: self.canvas.width(),
            height: self.canvas.height(),
        }
    }
}
