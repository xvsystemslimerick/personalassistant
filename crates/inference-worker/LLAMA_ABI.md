# llama.cpp ABI pin

The persistent backend must compile against the C API from the same upstream
revision as the bundled runtime. It must not use a floating package or system
installation.

- Release: `b10434`
- Commit: `7e4c0a968`
- Header: `https://github.com/ggml-org/llama.cpp/blob/7e4c0a968/include/llama.h`
- Header SHA-256: `1fbcba4003cfc089fa9681973acb3d9e24e465f32c3506ceb0ec7fa29952bdae`
- Reference example: `https://github.com/ggml-org/llama.cpp/blob/7e4c0a968/examples/simple/simple.cpp`
- Example SHA-256: `9a5bdba74c80ec545c52038e331a9a75b63fa87be57498f21fe4a62794ac4943`

The build must verify these digests before compiling the backend. ABI
structures returned by value may not be re-created from memory. Provider-data
activation remains prohibited until the persistent backend and packaged worker
pass the synthetic corpus and lifecycle tests.

The official header compiled for Apple Silicon reports these ABI sizes:

- `llama_model_params`: 72 bytes
- `llama_context_params`: 160 bytes
- `llama_sampler_chain_params`: 1 byte
- `llama_batch`: 56 bytes

The worker C shim carries compile-time guards for these values and probes every
runtime symbol required by the persistent generation loop before model access.

An isolated prototype demonstrated that matching structure sizes and offsets is
not sufficient to guarantee safe by-value calls across this ABI. The complete
seven-header dependency set is therefore vendored and digest-checked by
`build.rs`; the shim includes those headers directly. The backend remains
disabled by default until the packaged Apple Metal corpus passes.
