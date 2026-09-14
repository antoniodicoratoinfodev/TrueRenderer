/// Argument the broker passes only when it has confined this process itself.
/// It is a claim, never a permission: `serve_confined` refuses unless the
/// kernel agrees that the confinement is actually in place.
const CONFINED: &str = "--confined";

fn main() {
    let confined = std::env::args().any(|a| a == CONFINED);
    let (input, output) = (std::io::stdin().lock(), std::io::stdout().lock());
    let served = if confined {
        tr_worker::serve_confined(input, output)
    } else {
        tr_worker::serve(input, output)
    };
    if let Err(error) = served {
        eprintln!("tr-worker: {error:#}");
    }
}
