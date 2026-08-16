use std::fs;
use std::path::PathBuf;

fn workspace_file(path: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read web distribution file {}: {error}",
            path.display()
        )
    })
}

#[test]
fn web_release_explicitly_skips_the_external_wasm_opt_pass() {
    let index = workspace_file("index.html");

    assert!(
        index.contains("data-trunk rel=\"rust\" data-bin=\"sekai\" data-wasm-opt=\"0\""),
        "Trunk's documented zero level must disable the crash-prone external wasm-opt pass"
    );
}

#[test]
fn service_worker_fetches_current_assets_before_offline_fallback() {
    let worker = workspace_file("assets/sw.js");
    let compact = worker.split_whitespace().collect::<String>();
    let network = worker
        .find("fetch(e.request")
        .expect("the worker must try the network for the current release");
    let fallback = worker
        .find("caches.match(e.request)")
        .expect("the worker must retain an offline cache fallback");

    assert!(worker.contains("'./sekai.js'"));
    assert!(worker.contains("'./sekai_bg.wasm'"));
    assert!(!worker.contains("eframe_template"));
    assert!(compact.contains("self.skipWaiting()"));
    assert!(compact.contains("self.clients.claim()"));
    assert!(compact.contains("caches.keys()"));
    assert!(
        network < fallback,
        "cache-first would let an obsolete wasm bundle hide a current release"
    );
}

#[test]
fn page_forces_service_worker_update_and_reloads_on_controller_change() {
    let index = workspace_file("index.html");

    assert!(index.contains("updateViaCache: 'none'"));
    assert!(index.contains("registration.update()"));
    assert!(index.contains("controllerchange"));
    assert!(index.contains("window.location.reload()"));
}
