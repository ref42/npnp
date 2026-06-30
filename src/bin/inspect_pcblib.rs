use std::env;
use std::fs::File;
use std::io::Read;

fn main() {
    let path = env::args().nth(1).expect("path");
    let file = File::open(&path).expect("open file");
    let mut compound = cfb::CompoundFile::open(file).expect("open cfb");
    println!("FILE {path}");
    visit(&mut compound, "/");
}

fn visit(compound: &mut cfb::CompoundFile<File>, dir: &str) {
    let entries: Vec<(String, bool)> = compound
        .read_storage(dir)
        .expect("read storage")
        .map(|entry| (entry.name().to_string(), entry.is_stream()))
        .collect();
    for (name, is_stream) in entries {
        let path = if dir == "/" {
            format!("/{name}")
        } else {
            format!("{dir}/{name}")
        };
        if is_stream {
            let mut stream = compound.open_stream(&path).expect("open stream");
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).expect("read stream");
            let preview: Vec<String> = buf.iter().take(24).map(|b| format!("{b:02X}")).collect();
            println!(
                "STREAM {path} len={} bytes={}",
                buf.len(),
                preview.join(" ")
            );
        } else {
            println!("STORAGE {path}");
            visit(compound, &path);
        }
    }
}
