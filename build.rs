fn main() {
    println!("cargo:rerun-if-changed=assets/css/style.css");
    println!("cargo:rerun-if-changed=templates/");
}
