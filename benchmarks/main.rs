use lithair_benchmarks::performance_benchmark::RaftstoneBenchmark;

fn main() {
    let mut benchmark = RaftstoneBenchmark::new();
    benchmark.run_full_suite();
}