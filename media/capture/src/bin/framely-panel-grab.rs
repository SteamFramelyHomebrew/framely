fn main() { if let Err(e) = framely_capture::grab::serve(&std::path::PathBuf::from("/dev/dri/card0")) { eprintln!("{e:#}"); std::process::exit(1); } }
