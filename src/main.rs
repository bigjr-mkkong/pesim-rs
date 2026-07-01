use pesim_rs::{pesim_burst_size, pesim_free, pesim_new};

fn main() {
    let sim_body = pesim_new();
    println!("Burst size is: {}", pesim_burst_size(sim_body));
    pesim_free(sim_body);
}
