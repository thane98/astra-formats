use astra_formats::Bundle;

fn main() {
    let bundle = Bundle::load("ubody_cel0af_c000.bundle").unwrap();
    bundle.save("tmp.bin").unwrap();
}