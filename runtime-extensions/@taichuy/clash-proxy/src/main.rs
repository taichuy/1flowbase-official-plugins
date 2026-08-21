use std::path::PathBuf;

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let flag = arguments.next();
    let path = arguments.next();
    if flag.as_deref() != Some(std::ffi::OsStr::new("--network-egress-config-file"))
        || path.is_none()
        || arguments.next().is_some()
    {
        eprintln!("clash-proxy requires exactly --network-egress-config-file <private-file>");
        std::process::exit(2);
    }
    if let Err(error) = clash_proxy_provider::run_stdio(&PathBuf::from(path.unwrap())) {
        eprintln!("clash-proxy failed: {error:#}");
        std::process::exit(1);
    }
}
