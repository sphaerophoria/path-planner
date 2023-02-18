use eframe::egui;

use common::Data;
use egui::{mutex::Mutex, text::LayoutJob, Color32, Style, TextEdit, TextStyle, Visuals};
use path_planner::{Color, PixelCoord, PixelOffset, Size};
use std::sync::Arc;

const DATA: &[u8] = include_bytes!("../../client/www/data.json");

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    let data: Data = serde_json::from_slice(DATA).expect("Failed to parse data");

    eframe::run_native(
        "Path Planner",
        options,
        Box::new(move |cc| Box::new(MyApp::new(cc, data))),
    )
}

struct MyApp {
    /// Behind an `Arc<Mutex<…>>` so we can pass it to [`egui::PaintCallback`] and paint later.
    path_planner: Arc<Mutex<path_planner::App>>,
    enable_path_debug: bool,
    next_regex: String,
    highlight_list: Vec<(String, Color)>,
}

impl MyApp {
    fn new(cc: &eframe::CreationContext<'_>, data: Data) -> Self {
        let gl = cc
            .gl
            .as_ref()
            .expect("You need to run eframe with the glow backend");

        cc.egui_ctx.set_style(Style {
            visuals: Visuals::dark(),
            ..Default::default()
        });
        let planner = path_planner::App::new(Arc::clone(gl), data).unwrap();
        Self {
            path_planner: Arc::new(Mutex::new(planner)),
            enable_path_debug: false,
            next_regex: String::new(),
            highlight_list: Vec::new(),
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut self.enable_path_debug, "Enable path debugging")
                    .changed()
                {
                    self.path_planner
                        .lock()
                        .set_debug_mode(self.enable_path_debug);
                }
            });
        });

        let response = egui::CentralPanel::default().show(ctx, |ui| {
            let (cursor_delta, cursor_position, cursor_down, scroll_delta) = ui.input(|i| {
                (
                    i.pointer.delta(),
                    i.pointer.interact_pos(),
                    i.pointer.primary_down(),
                    i.scroll_delta,
                )
            });

            let map_rect = ui.max_rect();

            let cursor_position = cursor_position.map(|pos| PixelCoord {
                x: pos.x - map_rect.min.x,
                y: pos.y - map_rect.min.y,
            });

            let viewport_size = Size {
                width: map_rect.width() as u32,
                height: map_rect.height() as u32,
            };

            // Clone locals so we can move them into the paint callback:
            let path_planner = self.path_planner.clone();

            if cursor_down {
                path_planner.lock().move_map(
                    &PixelOffset {
                        x: -cursor_delta.x,
                        y: -cursor_delta.y,
                    },
                    &viewport_size,
                );
            }

            if let Some(cursor_position) = cursor_position.as_ref() {
                let zoom_amount = f32::powf(1.003, scroll_delta.y);
                path_planner
                    .lock()
                    .zoom(zoom_amount, cursor_position, &viewport_size);
                path_planner
                    .lock()
                    .update_cursor_pos(Some(cursor_position), &viewport_size);
            } else {
                path_planner.lock().update_cursor_pos(None, &viewport_size);
            }

            let callback = egui::PaintCallback {
                rect: map_rect,
                callback: std::sync::Arc::new(egui_glow::CallbackFn::new(
                    move |_info, _painter| {
                        path_planner.lock().render_map();
                    },
                )),
            };
            ui.painter().add(callback);

            let path_planner = self.path_planner.lock();

            let mut info_text = String::new();

            for tag in path_planner.selected_tags() {
                info_text += tag;
                info_text += "\n";
            }

            if let Some(cursor_position) = cursor_position.as_ref() {
                let geo_coord = path_planner.pixel_to_geocoord(cursor_position, &viewport_size);
                info_text += &format!("Lat: {}\nLong: {}", geo_coord.lat, geo_coord.long);
            }

            let rect_width = ui.max_rect().width() / 4.0;

            let info_layout_job = LayoutJob::simple(
                info_text,
                ui.style().text_styles[&TextStyle::Body].clone(),
                ui.style().visuals.text_color(),
                rect_width,
            );

            let info_galley = ui.fonts(move |f| f.layout_job(info_layout_job));

            let mut height = info_galley.rect.height();

            // FIXME: Figure out how to find the right number
            height += (ui.text_style_height(&TextStyle::Button)
                + ui.spacing().item_spacing.y * 3.0)
                * (self.highlight_list.len() + 1) as f32;

            let mut rect = ui.max_rect();
            let border_padding = 20.0;
            rect.max.x -= border_padding;
            rect.max.y -= border_padding;
            rect.min.x = rect.max.x - rect_width;
            rect.min.y = rect.max.y - height;

            ui.allocate_ui_at_rect(rect, |ui| {
                let mut paint_rect = rect;
                let item_spacing = ui.spacing().item_spacing;
                paint_rect.min.x -= item_spacing.x;
                paint_rect.min.y -= item_spacing.y;
                paint_rect.max.x += item_spacing.x;
                paint_rect.max.y += item_spacing.y;
                ui.painter()
                    .rect_filled(paint_rect, 0.0, Color32::from_black_alpha(200));

                ui.allocate_space(info_galley.rect.size());
                ui.painter().galley(ui.min_rect().left_top(), info_galley);

                let mut highlight_list_changed = false;

                let mut to_delete = None;
                for (i, item) in self.highlight_list.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(&item.0);

                        let mut colors = [item.1.r, item.1.g, item.1.b];
                        highlight_list_changed |= ui.color_edit_button_rgb(&mut colors).changed();
                        item.1.r = colors[0];
                        item.1.g = colors[1];
                        item.1.b = colors[2];

                        if ui.button("Del").clicked() {
                            to_delete = Some(i);
                            highlight_list_changed = true;
                        }
                    });
                }

                if let Some(to_delete) = to_delete {
                    self.highlight_list.remove(to_delete);
                }

                ui.horizontal(|ui| {
                    TextEdit::singleline(&mut self.next_regex)
                        .desired_width(100.0)
                        .show(ui);

                    if ui.button("Add").clicked() {
                        self.highlight_list.push((
                            std::mem::take(&mut self.next_regex),
                            Color::from_rgb(0.0, 0.0, 0.0),
                        ));
                        highlight_list_changed = true;
                    }
                });

                if highlight_list_changed {
                    let _ = path_planner.set_highlight_list(&self.highlight_list);
                }
            });
        });

        response.response.context_menu(|ui| {
            if ui.button("Start path").clicked() {
                self.path_planner.lock().start_path_plan();
                ui.close_menu();
            };

            if ui.button("Clear path").clicked() {
                self.path_planner.lock().clear_path_plan();
                ui.close_menu();
            };
        });
    }
}
