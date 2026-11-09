// Prints "working" forever and reads nothing: a terminal that looks busy and never needs a model.
use std::io::Write;
fn main(){ loop { println!("working"); std::io::stdout().flush().ok(); std::thread::sleep(std::time::Duration::from_millis(700)); } }
