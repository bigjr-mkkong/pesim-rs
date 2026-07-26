use pesim_rs::{PESim_config, pesim_burst_size, pesim_free, pesim_new};
use std::ffi::CString;

fn main() {
    let config_file = CString::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/cfg/DDR4_8Gb_x4_2400.ini"
    ))
    .unwrap();
    let output_dir = CString::new(concat!(env!("CARGO_MANIFEST_DIR"), "/output")).unwrap();
    let config = PESim_config {
        config_file: config_file.as_ptr(),
        output_dir: output_dir.as_ptr(),
        controller_id: 0,
        controller_base: 0x8000_0000,
        controller_size: 8 * 1024 * 1024 * 1024,
        pim_size: 0,
    };
    let sim_body = pesim_new(&config);
    println!("Burst size is: {}", pesim_burst_size(sim_body));
    pesim_free(sim_body);
}
