use tempfile::tempdir;
fn main() {
    let dir = tempdir().unwrap();
    let path = dir.keep().unwrap();
    println!("{:?}", path);
}
