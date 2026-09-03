use inference_worker::backend::PersistentBackend;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let runtime = args.next().ok_or("missing runtime directory")?;
    let model = args.next().ok_or("missing model path")?;
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let source =
        "Invoice INV-204 is attached. Please approve it and send the signed form by Friday.";
    let mut backend = PersistentBackend::open(Path::new(&runtime), Path::new(&model), 0)?;
    let extraction = backend.extract(source)?;
    println!(
        "classification={:?} tasks={} appointments={} waiting={}",
        extraction.classification,
        extraction.tasks.len(),
        extraction.appointments.len(),
        extraction.waiting_for.len()
    );
    Ok(())
}
