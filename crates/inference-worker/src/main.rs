use std::io;

fn main() {
    // The worker intentionally has no command-line payloads, filesystem logs,
    // or network listener. Parent/child IPC is confined to inherited pipes.
    if run().is_err() {
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "persistent-backend-experimental")]
    if let (Some(runtime), Some(model)) = (
        std::env::var_os("PA_INFERENCE_RUNTIME_DIR"),
        std::env::var_os("PA_INFERENCE_MODEL_PATH"),
    ) {
        let gpu_layers = std::env::var("PA_INFERENCE_GPU_LAYERS")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(99);
        let mut backend = inference_worker::backend::PersistentBackend::open(
            std::path::Path::new(&runtime),
            std::path::Path::new(&model),
            gpu_layers,
        )?;
        return inference_worker::serve_persistent(
            &mut io::stdin().lock(),
            &mut io::stdout().lock(),
            &mut backend,
        )
        .map_err(Into::into);
    }
    inference_worker::serve(&mut io::stdin().lock(), &mut io::stdout().lock()).map_err(Into::into)
}
