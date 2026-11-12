fn main() {
    println!("cargo:rerun-if-changed=host.rc");
    println!("cargo:rerun-if-changed=host.manifest");
    println!("cargo:rerun-if-changed=assets/ui4me.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("host.rc", embed_resource::NONE)
            .manifest_required()
            .expect("Windows resource compilation failed");
    }
}
