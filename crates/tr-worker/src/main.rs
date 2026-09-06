fn main() {
    if let Err(error) = tr_worker::serve(std::io::stdin().lock(), std::io::stdout().lock()) {
        eprintln!("tr-worker: {error:#}");
    }
}
