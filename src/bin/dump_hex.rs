use std::env;
use std::fs::File;
use std::io::Read;

fn main() {
    let path = env::args().nth(1).unwrap();
    let stream_path = env::args().nth(2).unwrap();
    let offset: usize = env::args().nth(3).unwrap().parse().unwrap();
    let count: usize = env::args().nth(4).unwrap().parse().unwrap();
    let file = File::open(path).unwrap();
    let mut compound = cfb::CompoundFile::open(file).unwrap();
    let mut stream = compound.open_stream(&stream_path).unwrap();
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).unwrap();
    let end = (offset + count).min(buf.len());
    for (index, byte) in buf[offset..end].iter().enumerate() {
        println!("{:03}: {:02X}", offset + index, byte);
    }
}
