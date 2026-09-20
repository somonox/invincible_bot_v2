use four_wide_bot::rl::combo_solver::{Graph, HORIZON};
fn main() {
    let start = std::time::Instant::now();
    let graph = Graph::build();
    println!(
        "boards={} edges={} build={:?}",
        graph.boards.len(),
        graph
            .edges
            .iter()
            .flat_map(|e| e.iter())
            .map(Vec::len)
            .sum::<usize>(),
        start.elapsed()
    );
    println!(
        "one-preview winning states by residue: {:?}",
        graph.winning_with_one_preview()
    );
    let values = graph.continuation_values(HORIZON);
    let bytes = graph.encode(&values);
    let path = std::env::args().nth(1).expect("output file path required");
    std::fs::write(&path, &bytes).unwrap();
    println!(
        "horizon={HORIZON} bytes={} elapsed={:?} output={path}",
        bytes.len(),
        start.elapsed()
    );
}
