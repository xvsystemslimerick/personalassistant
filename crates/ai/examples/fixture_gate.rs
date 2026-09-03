use ai::{detect, evaluate_case, evaluation_corpus, run_fixture_extraction};
use std::{env, path::Path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = env::args_os().skip(1);
    let runtime = arguments
        .next()
        .ok_or("runtime directory argument is required")?;
    let models = arguments
        .next()
        .ok_or("model directory argument is required")?;
    let requested_case = arguments.next();
    if arguments.next().is_some() {
        return Err("unexpected argument".into());
    }
    let models = Path::new(&models);
    let artifact = detect(models)?.recommendation.artifact;
    let cases = evaluation_corpus();
    let case = if let Some(requested) = requested_case {
        let requested = requested.to_string_lossy();
        cases
            .iter()
            .find(|case| case.id == requested)
            .ok_or("unknown evaluation case")?
    } else {
        &cases[0]
    };
    let extraction = run_fixture_extraction(Path::new(&runtime), models, &artifact, case.source)?;
    println!("{}", serde_json::to_string_pretty(&extraction)?);
    if !evaluate_case(case, &extraction) {
        return Err("fixture failed the semantic quality gate".into());
    }
    Ok(())
}
