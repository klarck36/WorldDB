fn main() {
    tauri_build::build();
    println!("cargo:rerun-if-env-changed=WORLDDB_ODE_APP_BUILD_ID");
    let build_id =
        std::env::var("WORLDDB_ODE_APP_BUILD_ID").unwrap_or_else(|_| "development".to_owned());
    println!("cargo:rustc-env=WORLDDB_ODE_APP_BUILD_ID={build_id}");
}
