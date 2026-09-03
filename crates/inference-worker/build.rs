fn main() {
    println!("cargo:rerun-if-changed=src/llama_abi_probe.c");
    let include = "vendor/llama-b10434/include";
    verify_headers(include);
    cc::Build::new()
        .file("src/llama_abi_probe.c")
        .include(include)
        .warnings(true)
        .warnings_into_errors(true)
        .compile("pa_llama_abi_probe");
}

fn verify_headers(include: &str) {
    use sha2::{Digest, Sha256};
    const HEADERS: [(&str, &str); 7] = [
        (
            "llama.h",
            "1fbcba4003cfc089fa9681973acb3d9e24e465f32c3506ceb0ec7fa29952bdae",
        ),
        (
            "ggml.h",
            "725ca1d9670770f7ee7e1f736983ce332c4f9f27999a5472b61002ff1fa78ea5",
        ),
        (
            "ggml-cpu.h",
            "316279e004cdeb8e6ef78599acb602bf79a8abdf897fed9fd1914808c1518c6e",
        ),
        (
            "ggml-backend.h",
            "46d84cb998105f871240864fd0f55446939a2fe86c5c281afa63a010fb1f65a2",
        ),
        (
            "ggml-opt.h",
            "3586de1bc8a934b5c72339e2b6937b0641e8f149b512231e666f67de0736eea2",
        ),
        (
            "gguf.h",
            "e56714aab702e5ce62ee587a409643c08f7e93e8fbb77f48ef7cc85075f96fa4",
        ),
        (
            "ggml-alloc.h",
            "94e4cd069b9313b2ceb35dacec901981e0bb478d8bb31035b7126be091998c23",
        ),
    ];
    for (name, expected) in HEADERS {
        let path = std::path::Path::new(include).join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("missing pinned header {name}"));
        let actual = format!("{:x}", Sha256::digest(bytes));
        assert_eq!(actual, expected, "pinned header digest mismatch for {name}");
    }
}
