/// Windows PE stack reserve (`/STACK`), in bytes. It must equal
/// `ui4me_renderer::ENGINE_STACK_BYTES` (checked by a unit test in `src/main.rs`): the engine runs
/// on its own thread of that size, and the reserve gives anything still on the process main thread
/// the same budget (F-01). On x64 this reserves address space only; pages are committed on use.
const WINDOWS_STACK_RESERVE: u64 = 33554432;

fn main() {
    println!("cargo:rerun-if-changed=host.rc");
    println!("cargo:rerun-if-changed=host.manifest");
    println!("cargo:rerun-if-changed=assets/ui4me.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("host.rc", embed_resource::NONE)
            .manifest_required()
            .expect("Windows resource compilation failed");
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:{WINDOWS_STACK_RESERVE}");
    }
}
