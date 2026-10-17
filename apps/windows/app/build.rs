fn main() {
    // The release workflow exports the tag's commit date (yyyy-MM-dd); local builds get "".
    println!("cargo:rerun-if-env-changed=CLAUDE_DASHBOARD_RELEASE_DATE");
    let date = std::env::var("CLAUDE_DASHBOARD_RELEASE_DATE").unwrap_or_default();
    println!(
        "cargo:rustc-env=CLAUDE_DASHBOARD_RELEASE_DATE={}",
        date.trim()
    );
    slint_build::compile("ui/app.slint").unwrap();
}
