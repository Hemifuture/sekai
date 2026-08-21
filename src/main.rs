#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

#[cfg(not(target_arch = "wasm32"))]
const DEFAULT_NATIVE_VIEWPORT_INNER_SIZE: [f32; 2] = [1600.0, 900.0];
#[cfg(not(target_arch = "wasm32"))]
const ACCEPTANCE_NATIVE_VIEWPORT_INNER_SIZE: [f32; 2] = [1280.0, 720.0];

#[cfg(not(target_arch = "wasm32"))]
fn native_acceptance_viewport_requested(
    frame_stats: Option<&str>,
    acceptance_viewport: Option<&str>,
) -> bool {
    frame_stats == Some("1") && acceptance_viewport == Some("1280x720")
}

#[cfg(not(target_arch = "wasm32"))]
fn native_viewport_inner_size(
    frame_stats: Option<&str>,
    acceptance_viewport: Option<&str>,
) -> [f32; 2] {
    if native_acceptance_viewport_requested(frame_stats, acceptance_viewport) {
        ACCEPTANCE_NATIVE_VIEWPORT_INNER_SIZE
    } else {
        DEFAULT_NATIVE_VIEWPORT_INNER_SIZE
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_viewport_decorated(frame_stats: Option<&str>, acceptance_viewport: Option<&str>) -> bool {
    !native_acceptance_viewport_requested(frame_stats, acceptance_viewport)
}

#[cfg(not(target_arch = "wasm32"))]
fn native_viewport_app_id(
    frame_stats: Option<&str>,
    acceptance_viewport: Option<&str>,
) -> Option<&'static str> {
    native_acceptance_viewport_requested(frame_stats, acceptance_viewport)
        .then_some("sekai-frame-stats-acceptance")
}

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    use std::sync::Arc;

    use eframe::{egui_wgpu, wgpu};

    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let frame_stats = std::env::var("SEKAI_FRAME_STATS").ok();
    let frame_stats_viewport = std::env::var("SEKAI_FRAME_STATS_VIEWPORT").ok();
    let viewport_inner_size =
        native_viewport_inner_size(frame_stats.as_deref(), frame_stats_viewport.as_deref());
    let acceptance_viewport_requested =
        viewport_inner_size == ACCEPTANCE_NATIVE_VIEWPORT_INNER_SIZE;
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(viewport_inner_size)
        .with_decorations(native_viewport_decorated(
            frame_stats.as_deref(),
            frame_stats_viewport.as_deref(),
        ))
        .with_min_inner_size([800.0, 600.0])
        .with_icon(
            // NOTE: Adding an icon is optional
            eframe::icon_data::from_png_bytes(&include_bytes!("../assets/icon-256.png")[..])
                .expect("Failed to load icon"),
        );
    if let Some(app_id) =
        native_viewport_app_id(frame_stats.as_deref(), frame_stats_viewport.as_deref())
    {
        viewport = viewport.with_app_id(app_id);
    }
    let native_options = eframe::NativeOptions {
        vsync: true,
        persist_window: !acceptance_viewport_requested,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: Some(1),
            on_surface_error: Arc::new(|e| {
                println!("WGPU error: {e:?}");
                egui_wgpu::SurfaceErrorAction::SkipFrame
            }),
            wgpu_setup: egui_wgpu::WgpuSetup::default(),
            // wgpu_setup: egui_wgpu::WgpuSetup::CreateNew( {
            //     instance_descriptor: wgpu::InstanceDescriptor {
            //         backends: wgpu::Backends::all(),
            //         flags: wgpu::InstanceFlags::default(),
            //         backend_options: wgpu::BackendOptions::default(),
            //     },
            //     power_preference: wgpu::PowerPreference::HighPerformance,
            //     device_descriptor: Arc::new(|_adapter| wgpu::DeviceDescriptor {
            //         label: Some("egui-wgpu"),
            //         ..Default::default()
            //     }),
            // }),
        },
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Sekai - World Generator",
        native_options,
        Box::new(|cc| Ok(Box::new(sekai::TemplateApp::new(cc)))),
    )
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    // Redirect `log` message to `console.log` and friends:
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(sekai::TemplateApp::new(cc)))),
            )
            .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{native_viewport_app_id, native_viewport_decorated, native_viewport_inner_size};

    #[test]
    fn acceptance_viewport_is_only_enabled_with_the_frame_sampler() {
        assert_eq!(
            native_viewport_inner_size(Some("1"), Some("1280x720")),
            [1280.0, 720.0]
        );
        assert_eq!(
            native_viewport_inner_size(None, Some("1280x720")),
            [1600.0, 900.0]
        );
        assert_eq!(
            native_viewport_inner_size(Some("0"), Some("1280x720")),
            [1600.0, 900.0]
        );
        assert!(!native_viewport_decorated(Some("1"), Some("1280x720")));
        assert!(native_viewport_decorated(None, Some("1280x720")));
        assert_eq!(
            native_viewport_app_id(Some("1"), Some("1280x720")),
            Some("sekai-frame-stats-acceptance")
        );
        assert_eq!(native_viewport_app_id(None, Some("1280x720")), None);
    }

    #[test]
    fn default_and_malformed_acceptance_viewports_preserve_the_product_default() {
        assert_eq!(native_viewport_inner_size(None, None), [1600.0, 900.0]);
        assert_eq!(native_viewport_inner_size(Some("1"), None), [1600.0, 900.0]);
        assert_eq!(
            native_viewport_inner_size(Some("1"), Some("1280X720")),
            [1600.0, 900.0]
        );
        assert_eq!(
            native_viewport_inner_size(Some("1"), Some("1920x1080")),
            [1600.0, 900.0]
        );
    }
}
