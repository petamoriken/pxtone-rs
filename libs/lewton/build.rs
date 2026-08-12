fn main() {
	println!("cargo:rustc-check-cfg=cfg(cargo_c)");
}
