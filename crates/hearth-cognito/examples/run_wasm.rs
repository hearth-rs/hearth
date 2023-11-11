use hearth_core::{
    cargo_process_metadata,
    flue::{Permissions, TableSignal},
    process::ProcessMetadata,
    runtime::{RuntimeBuilder, RuntimeConfig},
};
use hearth_types::{registry::RegistryRequest, wasm::WasmSpawnInfo};
use tracing::info;

#[tokio::main]
async fn main() {
    hearth_core::init_logging();

    let wasm_path = std::env::args()
        .nth(1)
        .expect("expected path to .wasm file");
    let wasm_data = std::fs::read(wasm_path).unwrap();

    let config = RuntimeConfig {};

    let config_path = hearth_core::get_config_path();
    let config_file = hearth_core::load_config(&config_path).unwrap();
    let mut builder = RuntimeBuilder::new(config_file);
    builder.add_plugin(hearth_cognito::WasmPlugin::default());
    let runtime = builder.run(config).await;

    let wasm_lump = runtime.lump_store.add_lump(wasm_data.into()).await;
    let spawn_info = WasmSpawnInfo {
        lump: wasm_lump,
        entrypoint: None,
    };

    let meta = cargo_process_metadata!();
    let parent = runtime.process_factory.spawn(meta);
    let response = parent.borrow_group().create_mailbox().unwrap();
    let response_cap = response
        .export(Permissions::SEND, parent.borrow_table())
        .unwrap();

    // import a cap to the registry's mailbox into the parent process
    let table = parent.borrow_table();
    let registry_mb = runtime.registry.borrow_parent();
    let registry = registry_mb.export(Permissions::SEND, table).unwrap();

    let request = RegistryRequest::Get {
        name: "hearth.cognito.WasmProcessSpawner".to_string(),
    };

    registry
        .send(&serde_json::to_vec(&request).unwrap(), &[&response_cap])
        .await
        .unwrap();

    let spawner = response
        .recv(|signal| {
            let TableSignal::Message { mut caps, .. } = signal else {
                panic!("expected message, got {:?}", signal);
            };

            caps.remove(0)
        })
        .await
        .unwrap();

    let spawner = parent.borrow_table().wrap_handle(spawner).unwrap();

    spawner
        .send(
            &serde_json::to_vec(&spawn_info).unwrap(),
            &[&response_cap, &registry],
        )
        .await
        .unwrap();

    hearth_core::wait_for_interrupt().await;

    info!("Interrupt received; exiting runtime");
}
